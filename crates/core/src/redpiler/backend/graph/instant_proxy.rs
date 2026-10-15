//! Execute validated region instructions in place, retaining synchronous reentrancy.
use super::*;

impl GraphBackend {
    pub(super) fn deliver_instant_route(
        &mut self,
        route: RouteId,
        binding: (u32, u32),
        captured: Option<&[Option<ObservedSite>]>,
    ) -> Result<(), GraphExecutionError> {
        let (proxy_index, route_index) = (binding.0 as usize, binding.1 as usize);
        let proxy = &self.program.instant_proxies()[proxy_index];
        let count = proxy.routes[route_index].steps.len();
        for index in 0..count {
            // Copy one instruction before dispatch; a nested walk may reenter this route.
            let step = self.program.instant_proxies()[proxy_index].routes[route_index].steps[index];
            match step.kind {
                InstantStepKind::Dispatch => self.deliver_callback(route, step.delivery, captured)?,
                InstantStepKind::Sample(actor) => {
                    if self.guard_matches(step.delivery.guard, captured) {
                        #[cfg(test)]
                        self.record_route_event(route, step.delivery, CallbackReceiver::PistonBase(actor));
                        self.sample_piston(actor)?;
                    }
                }
                InstantStepKind::ArmReset(reset) => {
                    // The certificate proves Always + Live + the reset's qualifying direction.
                    #[cfg(test)]
                    self.record_route_event(route, step.delivery, CallbackReceiver::Component(reset));
                    self.schedule(reset, 1, TickPriority::Normal);
                }
            }
        }
        Ok(())
    }
}
