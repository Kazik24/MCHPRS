//! Execute pure packed reads at the same sample/acceptance cuts as ordinary Graph.
use super::*;

impl GraphBackend {
    #[cfg(test)]
    pub(crate) fn use_general_cpu_predicates(&mut self) {
        self.program = self.program.clone().with_general_cpu_predicates();
    }

    #[cfg(test)]
    pub(crate) fn use_general_cpu_wire_reads(&mut self) {
        self.program = self.program.clone().with_general_cpu_wire_reads();
    }

    pub(super) fn cpu_predicate(&self, predicate: CpuPredicate) -> bool {
        self.program
            .cpu_kernels()
            .evaluate(predicate, |input| self.cpu_read(input), self.instant_kernel.clause_matcher)
    }

    pub(super) fn cpu_power_predicate(&self, predicate: CpuPredicate) -> bool {
        self.program
            .cpu_power_queries()
            .evaluate(predicate, |input| self.cpu_read(input), self.instant_kernel.clause_matcher)
    }

    pub(super) fn cpu_read(&self, input: CpuRead) -> bool {
        match input {
            CpuRead::State(test) => self.state.matches(test),
            CpuRead::Strength { node, threshold } => self.stored_component_strength(node) > threshold,
            CpuRead::WireEmission { node, face, threshold } => {
                let wire = self.wire_state(node);
                wire.power > threshold && self.wire_emits(node, wire, face)
            }
            CpuRead::Guard(guard) => self.guard_matches(guard, None),
            CpuRead::Source { site, port, threshold } => self.site_power(site, port) > threshold,
            CpuRead::Input { node, port } => self.input_strength(node, port) > 0,
        }
    }

    pub(super) fn piston_powered(&self, actor: PistonId) -> bool {
        if self.active_frame_retained_actors.contains(&actor) {
            let node = self.program.pistons()[actor.index()].node;
            return self.input_strength(node, InputPort::Main) > 0 || self.input_strength(node, InputPort::Quasi) > 0;
        }
        if self.fixed_classic.is_none()
            && let Some(entry) = self.program.cpu_power_query_entry(actor)
        {
            let powered = self.cpu_power_predicate(entry.main) || self.cpu_power_predicate(entry.quasi);
            #[cfg(test)]
            if self.test_recording {
                let node = self.program.pistons()[actor.index()].node;
                assert_eq!(
                    powered,
                    self.input_strength(node, InputPort::Main) > 0 || self.input_strength(node, InputPort::Quasi) > 0,
                    "compiled live piston power query"
                );
            }
            return powered;
        }
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

    #[cfg(test)]
    pub(crate) fn use_general_cpu_power_queries(&mut self) {
        self.program = self.program.clone().with_general_cpu_power_queries();
    }

    pub(super) fn run_cpu_kernel(&mut self, entry: CpuKernelEntry, request: PistonRequest) -> Result<bool, GraphExecutionError> {
        if !self.instant_event_ready(request.piston) {
            return Ok(false);
        }
        let accepted = !self.piston_powered(request.piston) && self.cpu_predicate(entry.valid);
        self.commit_instant_event(request, accepted)
    }
}
