//! Ordered actuator effects with shared payloads and live request revalidation.
use super::*;

impl Execution {
    /// This ready-entry equation admits only poses where an ordinary sample would accept no action.
    pub(crate) fn steady_actor_pose(&self, compiler: &mut Compiler<'_>, actor: PistonId) -> Result<Formula> {
        let powered = Self::powered(compiler, actor)?;
        let extended = compiler.state.actors[actor.index()].extended;
        let lane = self.lane_available(compiler, actor)?;
        let can_extend = compiler.logic.or(extended, lane)?;
        Ok(compiler.logic.and(powered, can_extend)?)
    }

    fn powered(compiler: &mut Compiler<'_>, actor: PistonId) -> Result<Formula> {
        let node = compiler.graph.pistons[actor.index()].node;
        let main = compiler.input_power(node, InputPort::Main)?;
        let quasi = compiler.input_power(node, InputPort::Quasi)?;
        let main = compiler.logic.powered(main)?;
        let quasi = compiler.logic.powered(quasi)?;
        Ok(compiler.logic.or(main, quasi)?)
    }

    pub(crate) fn sample(&mut self, compiler: &mut Compiler<'_>, actor: PistonId, when: Formula) -> Result<()> {
        let powered = Self::powered(compiler, actor)?;
        let extended = compiler.state.actors[actor.index()].extended;
        let pending = Self::base_pending(compiler, actor)?;
        let available = compiler.logic.not(pending)?;
        let active = compiler.logic.and(when, available)?;
        let retracted = compiler.logic.not(extended)?;
        let extend = compiler.logic.and(powered, retracted)?;
        let lane = self.lane_available(compiler, actor)?;
        let extend = compiler.logic.and(extend, lane)?;
        let extend = compiler.logic.and(active, extend)?;
        self.work.request(
            compiler,
            Request {
                actor,
                action: PistonAction::Extend,
            },
            extend,
        )?;
        let unpowered = compiler.logic.not(powered)?;
        let retract = compiler.logic.and(unpowered, extended)?;
        let retract = compiler.logic.and(active, retract)?;
        let spec = compiler.graph.pistons[actor.index()];
        let early = match (Self::resource_for_actor(compiler, actor), spec.lane) {
            (Some(resource), PistonLane::SingleResource { far }) => self.pending_extension(compiler, resource, far, spec.facing, true)?,
            _ => Formula::FALSE,
        };
        let normal = compiler.logic.not(early)?;
        let normal = compiler.logic.and(retract, normal)?;
        let early = compiler.logic.and(retract, early)?;
        self.work.request(
            compiler,
            Request {
                actor,
                action: PistonAction::Retract,
            },
            normal,
        )?;
        self.work.request(
            compiler,
            Request {
                actor,
                action: PistonAction::RetractWithoutPull,
            },
            early,
        )?;
        Ok(())
    }

    pub(crate) fn accept(&mut self, compiler: &mut Compiler<'_>, event: Guarded<Request>) -> Result<()> {
        let request = event.value;
        let powered = Self::powered(compiler, request.actor)?;
        let previous_extended = compiler.state.actors[request.actor.index()].extended;
        let qualifies = if request.action == PistonAction::Extend {
            let retracted = compiler.logic.not(previous_extended)?;
            let ready = compiler.state.actors[request.actor.index()].availability[PistonAvailability::Ready as usize];
            let extend = compiler.logic.and(powered, retracted)?;
            let extend = compiler.logic.and(extend, ready)?;
            let lane = self.lane_available(compiler, request.actor)?;
            compiler.logic.and(extend, lane)?
        } else {
            let unpowered = compiler.logic.not(powered)?;
            let retract = compiler.logic.and(unpowered, previous_extended)?;
            let pending = Self::base_pending(compiler, request.actor)?;
            let available = compiler.logic.not(pending)?;
            let retract = compiler.logic.and(retract, available)?;
            let lane = self.retraction_available(compiler, request.actor, request.action == PistonAction::RetractWithoutPull)?;
            compiler.logic.and(retract, lane)?
        };
        let when = compiler.logic.and(event.when, qualifies)?;
        if when == Formula::FALSE {
            return Ok(());
        }
        if request.action == PistonAction::Extend {
            self.extend(compiler, request.actor, when)?;
        } else {
            self.retract(compiler, request.actor, request.action == PistonAction::RetractWithoutPull, when)?;
        }
        let accepted = Accepted {
            when,
            request,
            previous_extended,
            tick: self.tick,
        };
        compiler.record_effect(Effect::Accepted(accepted.clone()))?;
        self.accepted.push(accepted);
        Ok(())
    }

    fn transfer(
        compiler: &mut Compiler<'_>,
        resource: ResourceId,
        contact: ContactId,
        enabled: bool,
        when: Formula,
    ) -> Result<Option<ResourcePublication>> {
        let when = compiler.write_resource(resource, contact, enabled, when)?;
        Ok((when != Formula::FALSE).then(|| ResourcePublication {
            when,
            resource,
            contact,
            stamp: compiler.state.resources[resource.index()].revision.stamp(),
        }))
    }

    fn extend(&mut self, compiler: &mut Compiler<'_>, actor: PistonId, when: Formula) -> Result<()> {
        let spec = compiler.graph.pistons[actor.index()];
        let resource = Self::resource_for_actor(compiler, actor);
        let move_resource = resource.map(|resource| compiler.state.resources[resource.index()].at(spec.near));
        let stamp = compiler.revise_actor(actor, when)?;
        compiler.write_actor(actor, when, None, Some(Formula::TRUE), Some(PistonAvailability::HeadPending))?;
        let payload = match (resource, move_resource, spec.lane) {
            (Some(resource), Some(at_near), PistonLane::SingleResource { far }) => {
                let moving = compiler.logic.and(when, at_near)?;
                Self::transfer(compiler, resource, far, false, moving)?
            }
            _ => None,
        };
        let moved = payload.as_ref().map_or(Formula::FALSE, |event| event.when);
        let publication = Publication {
            when,
            actor_valid: Formula::TRUE,
            actor,
            stamp,
            resource: payload,
            due_tick: self.tick + 2,
            extending: true,
        };
        compiler.record_effect(Effect::Publication(publication.clone()))?;
        self.publications.push_back(publication);
        // The transfer must be visible to nested callbacks before any notification is expanded.
        if let Some(route) = spec.routes.far_shape_changed {
            compiler.route(route, moved, None, self)?;
        }
        compiler.route(spec.routes.near_changed, when, None, self)?;
        compiler.route(spec.routes.near_changed, moved, None, self)?;
        compiler.write_actor(actor, when, Some(Formula::TRUE), None, None)?;
        compiler.route(spec.routes.base_changed, when, None, self)
    }

    fn retract(&mut self, compiler: &mut Compiler<'_>, actor: PistonId, without_pull: bool, when: Formula) -> Result<()> {
        let spec = compiler.graph.pistons[actor.index()];
        let resource = Self::resource_for_actor(compiler, actor);
        let (restore, pull) = match (resource, spec.lane) {
            (Some(resource), PistonLane::SingleResource { far }) => {
                let restore = self.pending_extension(compiler, resource, far, spec.facing, false)?;
                let state = &compiler.state.resources[resource.index()];
                let pull = if without_pull {
                    Formula::FALSE
                } else {
                    compiler.logic.and(state.at(far), state.enabled)?
                };
                (compiler.logic.and(when, restore)?, compiler.logic.and(when, pull)?)
            }
            _ => (Formula::FALSE, Formula::FALSE),
        };
        let interrupted = compiler.state.actors[actor.index()].availability[PistonAvailability::HeadPending as usize];
        let normal = compiler.logic.not(interrupted)?;
        let interrupted = compiler.logic.and(when, interrupted)?;
        let normal = compiler.logic.and(when, normal)?;
        let stamp = compiler.revise_actor(actor, when)?;
        compiler.write_actor(actor, interrupted, None, None, Some(PistonAvailability::PublishingHeadRemoval))?;
        compiler.write_actor(actor, normal, None, None, Some(PistonAvailability::BasePendingHeadPresent))?;
        compiler.route(spec.routes.near_changed, interrupted, None, self)?;
        compiler.write_actor(actor, interrupted, None, Some(Formula::FALSE), Some(PistonAvailability::BasePending))?;
        compiler.route(spec.routes.base_changed, interrupted, None, self)?;
        compiler.route(spec.routes.base_changed, normal, None, self)?;
        compiler.write_actor(actor, normal, None, Some(Formula::FALSE), Some(PistonAvailability::BasePending))?;
        if let (Some(resource), PistonLane::SingleResource { far }) = (resource, spec.lane) {
            let changed = compiler.write_resource(resource, far, true, restore)?;
            if let Some(route) = spec.routes.resource_restored_far {
                compiler.route(route, changed, None, self)?;
            }
        }
        let payload = if let Some(resource) = resource {
            Self::transfer(compiler, resource, spec.near, false, pull)?
        } else {
            None
        };
        let moved = payload.as_ref().map_or(Formula::FALSE, |event| event.when);
        if let Some(route) = spec.routes.far_changed {
            compiler.route(route, moved, None, self)?;
        }
        compiler.route(spec.routes.near_changed, when, None, self)?;
        let publication = Publication {
            when,
            actor_valid: Formula::TRUE,
            actor,
            stamp,
            resource: payload,
            due_tick: self.tick + 2,
            extending: false,
        };
        compiler.record_effect(Effect::Publication(publication.clone()))?;
        self.publications.push_back(publication);
        Ok(())
    }

    fn publish_resource(&mut self, compiler: &mut Compiler<'_>, event: &Publication) -> Result<()> {
        let Some(payload) = &event.resource else {
            return Ok(());
        };
        let current = &compiler.state.resources[payload.resource.index()];
        let unchanged = current.revision.unchanged(payload.stamp, &mut compiler.logic)?;
        let at_contact = current.at(payload.contact);
        let disabled = compiler.logic.not(current.enabled)?;
        let valid = compiler.logic.and(unchanged, at_contact)?;
        let valid = compiler.logic.and(valid, disabled)?;
        let valid = compiler.logic.and(valid, payload.when)?;
        let valid = compiler.logic.and(valid, event.when)?;
        let available = self.resource_available(compiler, payload.resource, payload.contact, None)?;
        let valid = compiler.logic.and(valid, available)?;
        let changed = compiler.write_resource(payload.resource, payload.contact, true, valid)?;
        let spec = compiler.graph.pistons[event.actor.index()];
        let route = if event.extending {
            spec.routes.resource_restored_far
        } else {
            spec.routes.resource_restored_near
        };
        if let Some(route) = route {
            compiler.route(route, changed, None, self)?;
        }
        Ok(())
    }

    pub(crate) fn publish(&mut self, compiler: &mut Compiler<'_>, event: Publication) -> Result<()> {
        let unchanged = compiler.state.actors[event.actor.index()]
            .revision
            .unchanged(event.stamp, &mut compiler.logic)?;
        let valid = compiler.logic.and(event.when, unchanged)?;
        let valid = compiler.logic.and(valid, event.actor_valid)?;
        let spec = compiler.graph.pistons[event.actor.index()];
        if event.extending {
            self.publish_resource(compiler, &event)?;
            compiler.write_actor(event.actor, valid, None, Some(Formula::TRUE), Some(PistonAvailability::Ready))?;
            compiler.route(spec.routes.head_restored, valid, None, self)?;
        } else {
            compiler.write_actor(
                event.actor,
                valid,
                Some(Formula::FALSE),
                Some(Formula::FALSE),
                Some(PistonAvailability::Ready),
            )?;
            compiler.route(spec.routes.base_restored, valid, None, self)?;
            self.publish_resource(compiler, &event)?;
        }
        Ok(())
    }
}
