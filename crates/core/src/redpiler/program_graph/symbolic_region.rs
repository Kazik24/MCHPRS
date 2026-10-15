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
}

/// The two ordered callbacks owned by one reset clock.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ResetEvent {
    /// This event handles a directional or captured reset notification.
    Update,
    /// This event advances the existing reset clock at its scheduled deadline.
    Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventRoutine {
    /// These are the only source bindings reachable from this event's effect roots.
    pub(crate) used_reads: Box<[usize]>,
    /// This topological Boolean plan preserves ordered effect-root occurrences.
    pub(crate) plan: Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorRead {
    /// This evaluates this link's live guard, port-filtered source and attenuation together.
    Power(PowerLink),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResetRead {
    /// This reads current or explicitly captured reset power.
    Powered,
    /// This checks membership in the existing deduplicated scheduler.
    Pending,
    /// This records whether the notification direction or captured power qualifies.
    Qualifies,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicActorRoutine {
    /// This is the existing actuator state owner served by these event routines.
    pub(crate) actor: PistonId,
    /// These are only this actor's graph-bound event inputs.
    pub(crate) reads: Box<[ActorRead]>,
    /// These routines handle ordered request sampling and action acceptance.
    pub(crate) routines: [EventRoutine; 2],
}

impl SymbolicActorRoutine {
    pub(crate) fn estimated_heap_bytes(&self) -> usize {
        self.reads.len() * std::mem::size_of::<ActorRead>() + self.routines.iter().map(routine_bytes).sum::<usize>()
    }

    /// This maps ordered request roots to the existing FIFO action type.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicResetProgram {
    /// This identifies the one graph node owning the existing reset clock.
    pub(crate) node: NodeId,
    /// This preserves the observer direction used by the native update callback.
    pub(crate) facing: mchprs_blocks::BlockFace,
    /// These bindings provide only the reset clock's live or captured event reads.
    pub(crate) reads: Box<[ResetRead]>,
    /// These routines arm the clock and compute its next scheduled output state.
    pub(crate) routines: [EventRoutine; 2],
}

impl SymbolicResetProgram {
    pub(crate) fn estimated_heap_bytes(&self) -> usize {
        self.reads.len() * std::mem::size_of::<ResetRead>() + self.routines.iter().map(routine_bytes).sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicRegion {
    /// This identifies the original InstantRegion whose complete boundary is compiled below.
    pub(crate) region: usize,
    /// This boundary belongs to the entire actor/resource/reset ownership group.
    pub(crate) boundary: KernelBoundary,
    /// These actor routines share the boundary without cloning it for each actor.
    pub(crate) actors: Box<[std::sync::Arc<SymbolicActorRoutine>]>,
    /// These unique clock routines retain one owner when several actors share a reset.
    pub(crate) resets: Box<[std::sync::Arc<SymbolicResetProgram>]>,
}

/// Lowered event programs omit draft boundary rows whose ordinals change during lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicRegionProgram {
    /// This identifies the original instant-region owner for diagnostics and coverage counts.
    pub(crate) region: usize,
    /// These actor routines retain copied graph-bound power links and live state evaluators.
    pub(crate) actors: Box<[std::sync::Arc<SymbolicActorRoutine>]>,
    /// These unique reset routines retain their existing clock-node owners.
    pub(crate) resets: Box<[std::sync::Arc<SymbolicResetProgram>]>,
}

impl SymbolicRegionProgram {
    pub(crate) fn estimated_heap_bytes(&self) -> usize {
        self.actors.len() * std::mem::size_of::<std::sync::Arc<SymbolicActorRoutine>>()
            + self.resets.len() * std::mem::size_of::<std::sync::Arc<SymbolicResetProgram>>()
            + self.actors.iter().map(|actor| actor.estimated_heap_bytes()).sum::<usize>()
            + self.resets.iter().map(|reset| reset.estimated_heap_bytes()).sum::<usize>()
    }
}

/// One region that could not be compiled atomically and therefore stays on general Graph execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicRegionDecline {
    /// This is the original InstantRegion index, including earlier declined or empty regions.
    pub(crate) region: usize,
    /// This is the exact number of actor entries omitted with the region.
    pub(crate) actor_count: usize,
    /// This is the exact number of reset clocks omitted with the region.
    pub(crate) reset_count: usize,
    /// This names the unsupported event routine that caused the whole region to decline.
    pub(crate) reason: &'static str,
}

/// The compiler publishes complete programs and explicit whole-region declines together.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SymbolicRegionCompilation {
    /// These are regions for which every owned actor and reset routine was compiled.
    pub(crate) programs: Vec<SymbolicRegion>,
    /// These are regions left entirely to the ordinary Graph executor.
    pub(crate) declines: Vec<SymbolicRegionDecline>,
}

impl SymbolicRegion {
    pub(crate) fn estimated_heap_bytes(&self) -> usize {
        self.boundary.inputs.capacity() * std::mem::size_of::<KernelInput>()
            + self.boundary.state.capacity() * std::mem::size_of::<KernelStateSlot>()
            + self.boundary.outputs.capacity() * std::mem::size_of::<KernelOutput>()
            + self.boundary.effects.capacity() * std::mem::size_of::<RouteOccurrence>()
            + self.boundary.projection_roots.capacity() * std::mem::size_of::<SiteId>()
            + self.actors.len() * std::mem::size_of::<std::sync::Arc<SymbolicActorRoutine>>()
            + self.resets.len() * std::mem::size_of::<std::sync::Arc<SymbolicResetProgram>>()
            + self.actors.iter().map(|actor| actor.estimated_heap_bytes()).sum::<usize>()
            + self.resets.iter().map(|reset| reset.estimated_heap_bytes()).sum::<usize>()
    }
}

fn routine_bytes(routine: &EventRoutine) -> usize {
    routine.used_reads.len() * std::mem::size_of::<usize>() + routine.plan.estimated_heap_bytes()
}
