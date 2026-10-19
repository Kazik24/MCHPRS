//! Execute a precompiled pull wave; no wire walks, physical queries or runtime compilation.
use super::*;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct KernelExecution {
    /// These Boolean slots are allocated once for the largest admitted expression program.
    scratch: Vec<bool>,
    /// This replaces only synchronous computation routes while a wave is committing.
    pub(super) active: Option<Arc<InstantKernel>>,
}

impl KernelExecution {
    pub(super) fn new(program: &CircuitProgram) -> Self {
        let size = program
            .instant_kernels()
            .iter()
            .map(|kernel| kernel.program.expressions.len())
            .max()
            .unwrap_or(0);
        Self {
            scratch: vec![false; size],
            ..Self::default()
        }
    }
}

impl GraphBackend {
    pub(super) fn run_instant_kernel(&mut self, request: PistonRequest) -> Result<bool, GraphExecutionError> {
        if request.action != mchprs_world::PistonAction::Retract || self.instant_kernel.active.is_some() {
            return Ok(false);
        }
        let Some(kernel) = self.program.instant_kernel_for_actor(request.piston).cloned() else {
            return Ok(false);
        };
        let Some(entry) = request.source.filter(|entry| kernel.entries.binary_search(entry).is_ok()) else {
            return Ok(false);
        };
        if kernel.actors.iter().any(|actor| self.early_resource_retraction(*actor))
            || kernel.wires.iter().any(|&(node, expected)| {
                let wire = self.wire_state(node);
                wire.power != expected || crate::redstone::wire::is_dot(wire)
            })
        {
            return Ok(false);
        }
        let mut scratch = std::mem::take(&mut self.instant_kernel.scratch);
        let evaluated = kernel.program.evaluate(&mut scratch, |input| self.kernel_input(input, entry));
        let eligible = evaluated.is_ok() && scratch[kernel.stationary.0 as usize] && self.kernel_owns_queue(&kernel, &scratch, request);
        if !eligible {
            self.instant_kernel.scratch = scratch;
            return Ok(false);
        }
        // Nothing observable is changed until the full entry and queue ownership checks succeed.
        if kernel.actors.len() != 1 {
            self.future.piston_requests.clear();
        }
        self.instant_kernel.active = Some(kernel.clone());
        let result = self.commit_kernel_pulls(&kernel, &scratch, entry);
        self.instant_kernel.active = None;
        self.instant_kernel.scratch = scratch;
        result?;
        self.counts.instant_kernel_invocations += 1;
        Ok(true)
    }

    fn kernel_owns_queue(&self, kernel: &InstantKernel, values: &[bool], first: PistonRequest) -> bool {
        // A singleton replaces only the already-popped action; callbacks append behind
        // the untouched queue exactly as Graph does. Multi-actor waves still own it all.
        let mut actual = std::iter::once(first).chain(self.future.piston_requests.iter().copied());
        let mut matched = 0;
        for pull in kernel.pulls.iter().filter(|pull| pull.external && values[pull.requested.0 as usize]) {
            let Some(request) = actual.next() else { return false };
            if request.piston != pull.actor || request.action != mchprs_world::PistonAction::Retract || request.source != first.source {
                return false;
            }
            matched += 1;
        }
        if kernel.actors.len() == 1 {
            matched == 1
        } else {
            actual.next().is_none()
        }
    }

    fn commit_kernel_pulls(&mut self, kernel: &InstantKernel, values: &[bool], entry: RouteId) -> Result<(), GraphExecutionError> {
        for pull in &kernel.pulls {
            if values[pull.accepted.0 as usize] {
                let extended = self.state.pistons[pull.actor.index()].extended;
                self.retract_connections(pull.actor, false)?;
                self.accepted(
                    PistonRequest {
                        piston: pull.actor,
                        action: mchprs_world::PistonAction::Retract,
                        source: Some(entry),
                    },
                    extended,
                );
            }
        }
        Ok(())
    }

    pub(super) fn deliver_kernel_route(&mut self, route: &KernelRoute) -> Result<(), GraphExecutionError> {
        for &node in &route.clear_wires {
            let mut wire = self.wire_state(node);
            wire.power = 0;
            self.set_wire(node, wire);
        }
        for &delivery in &route.deliveries {
            self.deliver_callback(route.route, delivery, None)?;
        }
        Ok(())
    }

    fn kernel_input(&self, input: LayerInput, entry: RouteId) -> bool {
        match input.source {
            LayerInputSource::BoundaryEvent { occurrence, .. } => occurrence.route == entry,
            LayerInputSource::SourceAbove { site, port, threshold } => self.site_power(site, port) > threshold,
            LayerInputSource::PowerContribution { link, .. } | LayerInputSource::LocalResetOutput { link, .. } => {
                self.guard_matches(link.guard, None) && self.site_power(link.from, link.from_port) > link.attenuation
            }
            LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => match test {
                StateTest::PistonExtended(actor) => self.state.pistons[actor.index()].extended,
                StateTest::PistonHeadPresent(actor) => self.state.pistons[actor.index()].head_present,
                StateTest::PistonAvailability { piston, is } => self.state.pistons[piston.index()].availability == is,
                StateTest::ResourceAt { resource, contact } => self.state.resources[resource.index()].at == contact,
                StateTest::ResourceEnabled(resource) => self.state.resources[resource.index()].enabled,
            },
        }
    }
}
