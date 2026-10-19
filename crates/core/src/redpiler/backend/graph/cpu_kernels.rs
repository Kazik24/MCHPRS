//! Execute pure packed reads at the same sample/acceptance cuts as ordinary Graph.
use super::*;

impl GraphBackend {
    pub(super) fn cpu_predicate(&self, predicate: CpuPredicate) -> bool {
        self.program.cpu_kernels().evaluate(
            predicate,
            |input| match input {
                CpuRead::State(test) => self.state.matches(test),
                CpuRead::Strength { node, threshold } => self.stored_component_strength(node) > threshold,
                CpuRead::Guard(guard) => self.guard_matches(guard, None),
                CpuRead::Source { site, port, threshold } => self.site_power(site, port) > threshold,
                CpuRead::Input { node, port } => self.input_strength(node, port) > 0,
            },
            self.instant_kernel.clause_matcher,
        )
    }

    pub(super) fn piston_powered(&self, actor: PistonId) -> bool {
        if let Some(entry) = self.program.cpu_kernel_entry(actor) {
            let powered = self.cpu_predicate(entry.main) || self.cpu_predicate(entry.quasi);
            #[cfg(test)]
            if self.test_recording {
                let node = self.program.pistons()[actor.index()].node;
                assert_eq!(
                    powered,
                    self.input_strength(node, InputPort::Main) > 0 || self.input_strength(node, InputPort::Quasi) > 0,
                    "packed live piston power"
                );
            }
            return powered;
        }
        let node = self.program.pistons()[actor.index()].node;
        self.input_strength(node, InputPort::Main) > 0 || self.input_strength(node, InputPort::Quasi) > 0
    }

    pub(super) fn run_cpu_kernel(&mut self, entry: CpuKernelEntry, request: PistonRequest) -> Result<bool, GraphExecutionError> {
        if !self.instant_event_ready(request.piston) {
            return Ok(false);
        }
        let accepted = !(self.cpu_predicate(entry.main) || self.cpu_predicate(entry.quasi)) && self.cpu_predicate(entry.valid);
        self.commit_instant_event(request, accepted)
    }
}
