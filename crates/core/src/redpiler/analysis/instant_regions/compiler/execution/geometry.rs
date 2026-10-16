//! Turn bounded contact occupancy into Boolean conditions before kernel execution.
use super::*;
use mchprs_blocks::block_entities::ContainerType;

impl Execution {
    pub(super) fn resource_for_actor(compiler: &Compiler<'_>, actor: PistonId) -> Option<ResourceId> {
        let spec = compiler.graph.pistons[actor.index()];
        let PistonLane::SingleResource { far } = spec.lane else {
            return None;
        };
        compiler
            .graph
            .resources
            .iter()
            .position(|resource| resource.contacts.contains(&spec.near) && resource.contacts.contains(&far))
            .map(|index| ResourceId::new(index as u32))
    }

    fn contact(compiler: &mut Compiler<'_>, contact: ContactId) -> Result<SiteView> {
        let site = compiler
            .graph
            .sites
            .iter()
            .position(|site| site.source == SiteSource::Contact(contact))
            .expect("every certified contact has a site");
        compiler.observe(SiteId::new(site as u32))
    }

    pub(super) fn base_pending(compiler: &mut Compiler<'_>, actor: PistonId) -> Result<Formula> {
        let state = &compiler.state.actors[actor.index()];
        Ok(compiler.logic.or(
            state.availability[PistonAvailability::BasePendingHeadPresent as usize],
            state.availability[PistonAvailability::BasePending as usize],
        )?)
    }

    pub(super) fn resource_available(
        &self,
        compiler: &mut Compiler<'_>,
        resource: ResourceId,
        contact: ContactId,
        vacated_head: Option<PistonId>,
    ) -> Result<Formula> {
        if !compiler.graph.resources[resource.index()].contacts.contains(&contact) {
            return Ok(Formula::FALSE);
        }
        let mut obstruction = Formula::FALSE;
        for (index, state) in compiler.state.resources.iter().enumerate() {
            if index != resource.index() {
                obstruction = compiler.logic.or(obstruction, state.at(contact))?;
            }
        }
        let graph = compiler.graph;
        for (index, spec) in graph.pistons.iter().enumerate() {
            let actor = PistonId::new(index as u32);
            if spec.base == contact {
                let pending = Self::base_pending(compiler, actor)?;
                let stationary = compiler.logic.not(pending)?;
                obstruction = compiler.logic.or(obstruction, stationary)?;
            }
            if spec.near == contact && vacated_head != Some(actor) {
                let state = &compiler.state.actors[index];
                let present_phase = compiler.logic.or(
                    state.availability[PistonAvailability::Ready as usize],
                    state.availability[PistonAvailability::HeadPending as usize],
                )?;
                let present_phase = compiler
                    .logic
                    .or(present_phase, state.availability[PistonAvailability::BasePendingHeadPresent as usize])?;
                let present = compiler.logic.and(state.head, present_phase)?;
                obstruction = compiler.logic.or(obstruction, present)?;
            }
        }
        let view = Self::contact(compiler, contact)?;
        let air = compiler.site_test(&view, |variant| variant.block == Block::Air)?;
        let state = compiler.state.resources[resource.index()].clone();
        let stationary = compiler.logic.and(state.at(contact), state.enabled)?;
        let mut free = compiler.logic.or(air, stationary)?;
        if vacated_head.is_some_and(|actor| graph.pistons[actor.index()].near == contact) {
            free = Formula::TRUE;
        }
        let clear = compiler.logic.not(obstruction)?;
        Ok(compiler.logic.and(clear, free)?)
    }

    pub(super) fn lane_available(&self, compiler: &mut Compiler<'_>, actor: PistonId) -> Result<Formula> {
        let spec = compiler.graph.pistons[actor.index()];
        let near = Self::contact(compiler, spec.near)?;
        let admissible = compiler.site_test(&near, |variant| {
            ContainerType::from_block(variant.block).is_none() && !matches!(variant.block, Block::MovingPiston { .. } | Block::PistonHead { .. })
        })?;
        let air = compiler.site_test(&near, |variant| variant.block == Block::Air)?;
        let mut occupied = Formula::FALSE;
        for resource in &compiler.state.resources {
            occupied = compiler.logic.or(occupied, resource.at(spec.near))?;
        }
        let empty = compiler.logic.not(occupied)?;
        let mut available = compiler.logic.and(air, empty)?;
        if let (Some(resource), PistonLane::SingleResource { far }) = (Self::resource_for_actor(compiler, actor), spec.lane) {
            let state = &compiler.state.resources[resource.index()];
            let present = compiler.logic.and(state.at(spec.near), state.enabled)?;
            let material = Block::from_id(compiler.graph.profiles[compiler.graph.resources[resource.index()].material.index()].raw_state);
            let material_matches = compiler.site_test(&near, |variant| variant.block == material)?;
            let present = compiler.logic.and(present, material_matches)?;
            let far_free = self.resource_available(compiler, resource, far, None)?;
            let push = compiler.logic.and(present, far_free)?;
            available = compiler.logic.or(available, push)?;
        }
        Ok(compiler.logic.and(admissible, available)?)
    }

    pub(super) fn pending_extension(
        &self,
        compiler: &mut Compiler<'_>,
        resource: ResourceId,
        contact: ContactId,
        facing: BlockFace,
        early_only: bool,
    ) -> Result<Formula> {
        let state = compiler.state.resources[resource.index()].clone();
        let disabled = compiler.logic.not(state.enabled)?;
        let present = compiler.logic.and(state.at(contact), disabled)?;
        let mut matched = Formula::FALSE;
        if compiler.region.is_some() {
            let queued = compiler.region_input(RegionRead::PendingExtension {
                resource,
                contact,
                facing,
                early_only,
            })?;
            let unchanged = state.revision.unchanged(0, &mut compiler.logic)?;
            matched = compiler.logic.and(queued, unchanged)?;
        }
        for event in &self.publications {
            if !event.extending
                || compiler.graph.pistons[event.actor.index()].facing != facing
                || (early_only && event.due_tick <= self.tick && !self.early_phase)
            {
                continue;
            }
            let Some(payload) = &event.resource else {
                continue;
            };
            if payload.resource != resource || payload.contact != contact {
                continue;
            }
            let valid = state.revision.unchanged(payload.stamp, &mut compiler.logic)?;
            let valid = compiler.logic.and(valid, payload.when)?;
            let valid = compiler.logic.and(valid, event.when)?;
            matched = compiler.logic.or(matched, valid)?;
        }
        Ok(compiler.logic.and(present, matched)?)
    }

    pub(super) fn retraction_available(&self, compiler: &mut Compiler<'_>, actor: PistonId, without_pull: bool) -> Result<Formula> {
        let spec = compiler.graph.pistons[actor.index()];
        let (Some(resource), PistonLane::SingleResource { far }) = (Self::resource_for_actor(compiler, actor), spec.lane) else {
            return Ok(Formula::TRUE);
        };
        let state = &compiler.state.resources[resource.index()];
        let at_far = state.at(far);
        let enabled = state.enabled;
        let matching = self.pending_extension(compiler, resource, far, spec.facing, false)?;
        let not_matching = compiler.logic.not(matching)?;
        let restore = self.resource_available(compiler, resource, far, None)?;
        let disabled_ok = compiler.logic.or(not_matching, restore)?;
        let enabled_ok = if without_pull {
            Formula::TRUE
        } else {
            self.resource_available(compiler, resource, spec.near, Some(actor))?
        };
        let selected = compiler.logic.select(enabled, enabled_ok, disabled_ok)?;
        Ok(compiler.logic.select(at_far, selected, Formula::TRUE)?)
    }
}
