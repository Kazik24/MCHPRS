//! Bounded, exact transition programs for closed instant selections.
use super::*;
use mchprs_world::PistonAction;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct MachineState {
    /// These packed values retain every admitted component's electrical or wire state.
    pub(crate) components: Vec<u16>,
    /// These poses retain extension, head presence and logical availability for every actor.
    pub(crate) pistons: Vec<u8>,
    /// These retain each conserved payload's selected contact and enabled state.
    pub(crate) resources: Vec<(ContactId, bool)>,
    /// These relative deadlines retain scheduler order, duplicates and priorities.
    pub(crate) ticks: Vec<MachineTick>,
    /// These retain FIFO requests awaiting action acceptance.
    pub(crate) requests: Vec<(PistonId, PistonAction)>,
    /// These retain deferred publications, including stale revision witnesses.
    pub(crate) deferred: Vec<MachineDeferred>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MachineTick {
    /// This identifies the compiled receiver with its original registry type.
    pub(crate) node: NodeId,
    /// This is the remaining half-tick delay relative to the scheduler cursor.
    pub(crate) delay: u32,
    /// This retains the tick's dispatch priority.
    pub(crate) priority: TickPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MachineDeferred {
    /// This identifies the actor whose connection change will be published.
    pub(crate) piston: PistonId,
    /// This is the captured revision minus the actor's current revision, modulo 2^64.
    pub(crate) revision: u64,
    /// This retains the payload publication and its independent revision witness.
    pub(crate) resource: Option<(ResourceId, u64, ContactId)>,
    /// This is the publication deadline relative to the logical tick, modulo 2^64.
    pub(crate) delay: u64,
    /// This distinguishes extension publication from retraction publication.
    pub(crate) extending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MachineTransition {
    /// This identifies the exact normalized state after the operation.
    pub(crate) next: u32,
    /// These component writes replace internal propagation and retain final wire shapes/strengths.
    pub(crate) components: Vec<(NodeId, u16)>,
    /// These actor writes retain the final pose and add the recorded revision increment.
    pub(crate) pistons: Vec<(PistonId, u8, u64)>,
    /// These payload writes retain location/enabled state and add its revision increment.
    pub(crate) resources: Vec<(ResourceId, ContactId, bool, u64)>,
    /// These nodes were touched even if their final value equals their entry value.
    pub(crate) dirty: Vec<NodeId>,
    /// These ordered piston animations remain queued for the next world projection.
    pub(crate) effects: Vec<(PistonId, PistonAction, bool)>,
    /// This retains the number of scheduled obligations discharged by this transition.
    pub(crate) scheduled: u64,
    /// This retains accepted logical piston actions without replaying their handlers.
    pub(crate) accepted: u64,
    /// This retains logical connection publications without replaying their notifications.
    pub(crate) published: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstantMachine {
    /// These lever owners define action columns 1..N; column zero advances one game tick.
    pub(crate) controls: Vec<NodeId>,
    /// These exact states retain all memory and future work needed to select a transition.
    pub(crate) states: Vec<MachineState>,
    /// These rows contain checked transitions; a missing entry retains ordinary Graph execution.
    pub(crate) transitions: Vec<Vec<Option<MachineTransition>>>,
    /// This complete-state lookup resolves hash collisions using structural equality.
    pub(crate) index: FxHashMap<MachineState, u32>,
}

impl InstantMachine {
    pub(crate) fn estimated_bytes(&self) -> usize {
        fn state_bytes(s: &MachineState) -> usize {
            s.components.capacity() * std::mem::size_of::<u16>()
                + s.pistons.capacity()
                + s.resources.capacity() * std::mem::size_of::<(ContactId, bool)>()
                + s.ticks.capacity() * std::mem::size_of::<MachineTick>()
                + s.requests.capacity() * std::mem::size_of::<(PistonId, PistonAction)>()
                + s.deferred.capacity() * std::mem::size_of::<MachineDeferred>()
        }
        self.controls.capacity() * std::mem::size_of::<NodeId>()
            + self.states.capacity() * std::mem::size_of::<MachineState>()
            + self.states.iter().map(state_bytes).sum::<usize>()
            + self.index.capacity() * (std::mem::size_of::<(MachineState, u32)>() + 1)
            + self.index.keys().map(state_bytes).sum::<usize>()
            + self.transitions.capacity() * std::mem::size_of::<Vec<Option<MachineTransition>>>()
            + self
                .transitions
                .iter()
                .map(|row| {
                    row.capacity() * std::mem::size_of::<Option<MachineTransition>>()
                        + row
                            .iter()
                            .flatten()
                            .map(|edge| {
                                edge.components.capacity() * std::mem::size_of::<(NodeId, u16)>()
                                    + edge.pistons.capacity() * std::mem::size_of::<(PistonId, u8, u64)>()
                                    + edge.resources.capacity() * std::mem::size_of::<(ResourceId, ContactId, bool, u64)>()
                                    + edge.dirty.capacity() * std::mem::size_of::<NodeId>()
                                    + edge.effects.capacity() * std::mem::size_of::<(PistonId, PistonAction, bool)>()
                            })
                            .sum::<usize>()
                })
                .sum::<usize>()
    }
}
