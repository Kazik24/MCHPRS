//! Guarded whole-episode programs: only original-cut external effects survive replay.
use super::*;
use std::sync::Arc;

const ABSENT: u32 = u32::MAX;
const MAX_OPS: usize = 65_536;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_VARIANTS: usize = 4;
const MAX_ATTEMPTS: u8 = 16;

#[derive(Default)]
pub(super) struct TransitionCache {
    nets: Vec<Vec<TransitionRoot>>,
    bytes: usize,
}

#[derive(Default)]
struct TransitionRoot {
    programs: Vec<Arc<TransitionProgram>>,
    attempts: u8,
}

pub(super) struct TransitionProgram {
    wire_reads: Box<[(u32, u8)]>,
    drives: Box<[(u32, u8)]>,
    geometry: Box<[(u32, u32)]>,
    ops: Box<[SignalOp]>,
    selections: u32,
    visits: u64,
    bytes: usize,
}

pub(super) enum TransitionEntry {
    Exact,
    Learn(TransitionRecording),
    Replay(Arc<TransitionProgram>),
}

#[derive(Default)]
pub(super) struct TransitionRecording {
    ops: Vec<SignalOp>,
    wire_reads: Vec<(u32, u8)>,
    seen_wires: FxHashSet<u32>,
    control_reads: FxHashSet<u32>,
    geometry: FxHashMap<u32, u32>,
    selected: Vec<bool>,
    pub(super) queues: [VecDeque<u32>; 3],
    disabled: bool,
}

impl TransitionRecording {
    pub(super) fn push(&mut self, op: SignalOp) {
        if self.disabled {
            return;
        }
        if self.ops.len() == MAX_OPS {
            self.disabled = true;
            self.ops.clear();
        } else {
            self.ops.push(op);
        }
    }

    pub(super) fn wire(&mut self, wire: u32, power: u8) {
        if !self.disabled && self.seen_wires.insert(wire) {
            self.wire_reads.push((wire, power));
            if self.wire_reads.len() > MAX_OPS {
                self.disabled = true;
            }
        }
    }

    pub(super) fn geometry(&mut self, wire: u32, inputs: u32) {
        if !self.disabled {
            if self.geometry.insert(wire, inputs).is_some_and(|old| old != inputs) {
                self.disabled = true;
            }
        }
    }

    pub(super) fn read_wire(&mut self, wire: u32) {
        if !self.disabled {
            self.control_reads.insert(wire);
        }
    }

    /// Rejected external slots remain placeholders, preserving their future call cut.
    pub(super) fn select(&mut self, wire: u32, slot: u8, admitted: bool) -> (u32, bool) {
        if self.disabled {
            return (ABSENT, admitted);
        }
        let selection = self.selected.len() as u32;
        self.selected.push(admitted);
        self.push(SignalOp::Select { wire, slot, selection });
        (selection, true)
    }

    pub(super) fn admitted(&self, selection: u32) -> bool {
        selection == ABSENT || self.selected[selection as usize]
    }
}

impl GraphBackend {
    pub(super) fn transition_entry(&mut self, index: u32, root: u32, net: &TransportNet) -> TransitionEntry {
        if self.program.instant_transition_net(index).is_none() || self.classic_proof.is_some() {
            return TransitionEntry::Exact;
        }
        #[cfg(test)]
        if self.test_recorder.wire_tapes.is_some() || self.test_recorder.value_region_census_enabled {
            return TransitionEntry::Exact;
        }
        let index = index as usize;
        if self.transition_cache.nets.len() <= index {
            self.transition_cache.nets.resize_with(index + 1, Vec::new);
        }
        if self.transition_cache.nets[index].is_empty() {
            self.transition_cache.nets[index].resize_with(net.wires.len(), TransitionRoot::default);
        }
        let count = self.transition_cache.nets[index][root as usize].programs.len();
        for variant in 0..count {
            let program = self.transition_cache.nets[index][root as usize].programs[variant].clone();
            if self.transition_matches(net, &program) {
                self.transition_cache.nets[index][root as usize].programs.swap(0, variant);
                return TransitionEntry::Replay(program);
            }
        }
        self.counts.instant_transition_misses += 1;
        let entry = &mut self.transition_cache.nets[index][root as usize];
        if entry.attempts == MAX_ATTEMPTS || self.transition_cache.bytes >= MAX_CACHE_BYTES {
            return TransitionEntry::Exact;
        }
        entry.attempts += 1;
        TransitionEntry::Learn(TransitionRecording::default())
    }

    fn transition_matches(&self, net: &TransportNet, program: &TransitionProgram) -> bool {
        program
            .wire_reads
            .iter()
            .all(|&(wire, power)| self.wire_state(net.wires[wire as usize].node).power == power)
            && program
                .drives
                .iter()
                .all(|&(query, power)| self.instant_drive_query_power(query) == power)
            && program
                .geometry
                .iter()
                .all(|&(wire, inputs)| self.transition_live_inputs(net, wire) == inputs)
    }

    fn transition_live_inputs(&self, net: &TransportNet, wire: u32) -> u32 {
        let plan = &net.wires[wire as usize];
        WireInputs {
            direct: plan.inputs,
            conditional: plan.conditional_inputs,
        }
        .selected(true, |slot| {
            let local = plan.neighbors[slot] as usize;
            if net.site_wire[local] != ABSENT {
                let block = Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                };
                (block.is_solid(), block.is_transparent())
            } else {
                let view = self.live_site(net.sites[local]);
                (view.solid, view.transparent)
            }
        })
    }

    pub(super) fn publish_transition(
        &mut self,
        index: u32,
        root: u32,
        net: &TransportNet,
        mut recording: TransitionRecording,
        sampled: &[u32],
        powers: &[u8],
        visits: u64,
    ) {
        if recording.disabled {
            return;
        }
        // Boundary reads stay live; only powers consumed by transport control constrain replay.
        recording.wire_reads.retain(|(wire, _)| recording.control_reads.contains(wire));
        let plan = self.program.instant_transition_net(index).unwrap();
        let mut drives = sampled
            .iter()
            .map(|&wire| (plan.wire_queries[wire as usize], powers[wire as usize]))
            .collect::<Vec<_>>();
        drives.sort_unstable();
        if drives.windows(2).any(|pair| pair[0].0 == pair[1].0 && pair[0].1 != pair[1].1) {
            return;
        }
        drives.dedup();
        let mut selections = recording.selected.len() as u32;
        minimize_signal_operations(net, &mut recording.ops, &mut selections, |wire, slot| {
            self.wire_plan(net.wires[wire as usize].node)
                .execution
                .callback_selection
                .and_then(|facts| (facts.known & (1 << slot) != 0).then_some(facts.admitted & (1 << slot) != 0))
        });
        let geometry = recording.geometry.into_iter().collect::<Vec<_>>().into_boxed_slice();
        let bytes = recording.ops.len() * std::mem::size_of::<SignalOp>()
            + recording.wire_reads.len() * std::mem::size_of::<(u32, u8)>()
            + drives.len() * std::mem::size_of::<(u32, u8)>()
            + geometry.len() * std::mem::size_of::<(u32, u32)>()
            + std::mem::size_of::<TransitionProgram>();
        let entry = &mut self.transition_cache.nets[index as usize][root as usize];
        let evicted = if entry.programs.len() == MAX_VARIANTS {
            entry.programs.last().unwrap().bytes
        } else {
            0
        };
        if self.transition_cache.bytes - evicted + bytes > MAX_CACHE_BYTES {
            return;
        }
        if evicted != 0 {
            entry.programs.pop();
        }
        entry.programs.insert(
            0,
            Arc::new(TransitionProgram {
                wire_reads: recording.wire_reads.into_boxed_slice(),
                drives: drives.into_boxed_slice(),
                geometry,
                ops: recording.ops.into_boxed_slice(),
                selections,
                visits,
                bytes,
            }),
        );
        self.transition_cache.bytes = self.transition_cache.bytes - evicted + bytes;
        self.counts.instant_transition_programs += 1;
    }

    pub(super) fn run_transition(&mut self, net: &TransportNet, program: &TransitionProgram) -> Result<(), GraphExecutionError> {
        self.counts.instant_transition_hits += 1;
        self.counts.instant_transition_operations += program.ops.len() as u64;
        self.counts.instant_transition_wire_visits_elided += program.visits;
        self.run_signal_operations(net, &program.ops, program.selections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_recording_budget_keeps_existing_placeholder_decisions_and_stops_learning() {
        let mut recording = TransitionRecording::default();
        let (accepted, _) = recording.select(0, 0, true);
        let (rejected, enqueue) = recording.select(0, 1, false);
        assert!(enqueue, "a recorded rejection needs a FIFO placeholder");
        for _ in 0..MAX_OPS {
            recording.push(SignalOp::Capture(0));
        }
        assert!(recording.disabled);
        assert!(recording.ops.is_empty());
        assert!(recording.admitted(accepted));
        assert!(!recording.admitted(rejected));
        assert_eq!(recording.select(0, 2, false), (ABSENT, false));
        assert_eq!(recording.select(0, 2, true), (ABSENT, true));
    }
}
