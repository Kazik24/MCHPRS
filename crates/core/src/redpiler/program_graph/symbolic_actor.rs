//! Immutable symbolic actor/reset event programs; state remains in CircuitState and GraphFuture.
use super::*;
use mchprs_redpiler_opt::Plan;
use mchprs_world::PistonAction;

pub(crate) const MAX_ACTOR_INPUTS: usize = 128;
pub(crate) const MAX_ACTOR_OPS: usize = 512;
pub(crate) const ACTOR_ROOTS: usize = 3;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ActorEvent {
    /// This event samples power and may enqueue one actuator request.
    Sample = 0,
    /// This event revalidates a queued action against the live state.
    Accept = 1,
    /// This event handles a directional or captured reset notification.
    ResetUpdate = 2,
    /// This event advances the existing reset clock at its scheduled deadline.
    ResetTick = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActorRoutine {
    /// These are the only source bindings reachable from this event's effect roots.
    pub(crate) used_reads: Box<[usize]>,
    /// This topological Boolean plan preserves ordered effect-root occurrences.
    pub(crate) plan: Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorRead {
    /// This samples the live, port-filtered source and applies this link's attenuation.
    Power(PowerLink),
    /// This evaluates the live geometry/state condition of a power contribution.
    Guard(GuardId),
    /// This reads the actor's current logical extension bit.
    Extended,
    /// This detects a base that cannot currently accept another sample.
    BasePending,
    /// This detects an actor eligible to accept an extension.
    Ready,
    /// This checks current head, payload and destination occupancy.
    LaneAvailable,
    /// This checks resource restoration or pulling for an ordinary retraction.
    RetractionAvailable,
    /// This checks resource restoration when the action does not pull.
    EarlyRetractionAvailable,
    /// This reads the pending extension direction and deadline deciding an early drop.
    EarlyResource,
    /// This reads current or explicitly captured reset power.
    ResetPowered,
    /// This checks membership in the existing deduplicated scheduler.
    ResetPending,
    /// This records whether the notification direction or captured power qualifies.
    ResetQualifies,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicActor {
    /// This is the existing actuator owner; there is no second copy of its state.
    pub(crate) actor: PistonId,
    /// This is the graph-owned observer or built-in clock supplying this actor's reset.
    pub(crate) reset: NodeId,
    /// This records the actual external ports and all retained reconstruction owners.
    pub(crate) boundary: KernelBoundary,
    /// These bindings supply expression inputs in declared order at each event.
    pub(crate) reads: Box<[ActorRead]>,
    /// These independent routines avoid reading actor power during an unrelated reset event.
    pub(crate) routines: [ActorRoutine; 4],
}

impl SymbolicActor {
    pub(crate) fn estimated_heap_bytes(&self) -> usize {
        self.reads.len() * std::mem::size_of::<ActorRead>()
            + self
                .routines
                .iter()
                .map(|routine| routine.used_reads.len() * std::mem::size_of::<usize>() + routine.plan.estimated_heap_bytes())
                .sum::<usize>()
            + self.boundary.inputs.capacity() * std::mem::size_of::<KernelInput>()
            + self.boundary.state.capacity() * std::mem::size_of::<KernelStateSlot>()
            + self.boundary.outputs.capacity() * std::mem::size_of::<KernelOutput>()
            + self.boundary.effects.capacity() * std::mem::size_of::<RouteOccurrence>()
            + self.boundary.projection_roots.capacity() * std::mem::size_of::<SiteId>()
    }
    /// Requests retain Extend, Retract and RetractWithoutPull occurrence order.
    pub(crate) fn requested(roots: &[bool; ACTOR_ROOTS]) -> Option<PistonAction> {
        Self::action(&roots[..3])
    }

    pub(crate) fn accepts(roots: &[bool; ACTOR_ROOTS], action: PistonAction) -> bool {
        roots[match action {
            PistonAction::Extend => 0,
            PistonAction::Retract => 1,
            PistonAction::RetractWithoutPull => 2,
        }]
    }

    fn action(roots: &[bool]) -> Option<PistonAction> {
        [PistonAction::Extend, PistonAction::Retract, PistonAction::RetractWithoutPull]
            .into_iter()
            .zip(roots)
            .find_map(|(action, &enabled)| enabled.then_some(action))
    }
}
