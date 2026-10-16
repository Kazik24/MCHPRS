//! Immutable symbolic actor/reset event programs; state remains in CircuitState and GraphFuture.
use super::*;
use mchprs_redpiler_opt::Plan;
use mchprs_world::PistonAction;

pub(crate) const MAX_ACTOR_INPUTS: usize = 128;
pub(crate) const MAX_ACTOR_OPS: usize = 512;
pub(crate) const ACTOR_ROOTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegionRead {
    /// This reads one live stored-strength bit from a component site.
    ComponentSignal { node: NodeId, bit: u8 },
    /// This reads the live powered flag of an external electrical component.
    ComponentPowered(NodeId),
    /// This reads the live lock flag of an external repeater.
    ComponentLocked(NodeId),
    /// This reads one bit of a compact encoding for a live wire side.
    WireSideBit { node: NodeId, side: u8, bit: u8 },
    /// This reads the live extension state of an external piston actor.
    ActorExtended(PistonId),
    /// This reads the live head-presence state of an external piston actor.
    ActorHead(PistonId),
    /// This reads one live availability state of an external piston actor.
    ActorAvailability { actor: PistonId, state: u8 },
    /// This reads whether a movable resource occupies one physical contact.
    ResourceAt { resource: ResourceId, contact: ContactId },
    /// This reads whether a movable resource is currently enabled.
    ResourceEnabled(ResourceId),
    /// This reads whether a local reset clock is already in the global scheduler.
    Pending(NodeId),
    /// This reports whether an incoming callback supplied captured block properties.
    CapturePresent(SiteId),
    /// This selects a captured profile and receiver pair when one profile has multiple receivers.
    CaptureAlternative {
        site: SiteId,
        profile: ProfileId,
        receiver: Option<CallbackReceiver>,
    },
    /// This reads captured powered state for an electrical callback operand.
    CapturePowered(SiteId),
    /// This reads captured lock state for a repeater callback operand.
    CaptureLocked(SiteId),
    /// This reads captured copper-bulb output state for a callback operand.
    CaptureLit(SiteId),
    /// This reads one bit of a compact encoding for a captured wire side.
    CaptureWireSideBit { site: SiteId, side: u8, bit: u8 },
    /// This reads one bit of captured wire power for a callback operand.
    CaptureWireSignalBit { site: SiteId, bit: u8 },
    /// This reads the captured extension bit for a piston contact operand.
    CaptureActorExtended(SiteId),
    /// This supplies the actor-revision witness result for one deferred event.
    DeferredActorValid(PistonId),
    /// This supplies the payload-revision witness result for one deferred event.
    DeferredResourceValid(ResourceId),
    /// This reads an already queued extension witness for payload eligibility.
    PendingExtension {
        resource: ResourceId,
        contact: ContactId,
        facing: BlockFace,
        early_only: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RegionStateSlot {
    /// This is one stored signal bit owned by a region component.
    ComponentSignal { node: NodeId, bit: u8 },
    /// This is the powered flag owned by an electrical component.
    ComponentPowered(NodeId),
    /// This is the repeater lock flag owned by an electrical component.
    ComponentLocked(NodeId),
    /// This is one stored wire up-side flag owned by a region wire.
    WireUp { node: NodeId, side: u8 },
    /// This is one stored wire flat-side flag owned by a region wire.
    WireFlat { node: NodeId, side: u8 },
    /// This is the extension bit owned by a region piston actor.
    ActorExtended(PistonId),
    /// This is head presence owned by a region piston actor.
    ActorHead(PistonId),
    /// This is one availability state owned by a region piston actor.
    ActorAvailability { actor: PistonId, state: u8 },
    /// This is occupancy of a region resource at one contact.
    ResourceAt { resource: ResourceId, contact: ContactId },
    /// This is the enabled state of a region resource.
    ResourceEnabled(ResourceId),
    /// This is one conditional actor-revision increment produced by the entry.
    ActorRevision { actor: PistonId, write: u32 },
    /// This is one conditional resource-revision increment produced by the entry.
    ResourceRevision { resource: ResourceId, write: u32 },
}

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
    /// This plan replaces complete owned callback propagation when regional tracing succeeds.
    pub(crate) event_kernel: Option<std::sync::Arc<SymbolicEventKernel>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SymbolicRegionEntry {
    /// This is an already-guarded callback entering an owned target with optional event capture.
    Callback {
        /// This is the original callback target whose handler is compiled into the entry.
        target: SiteId,
        /// This is the receiver selected by the original callback occurrence.
        receiver: CallbackReceiver,
        /// This preserves update/shape identity and its direction payload.
        kind: DeliveryKind,
        /// This preserves whether the callback supplied its captured target observation.
        captured: bool,
    },
    /// This is the direct actor sample performed by an incoming callback or proxy step.
    Sample(PistonId),
    /// This revalidates one action already present in the global request FIFO.
    Accept { actor: PistonId, action: mchprs_world::PistonAction },
    /// This advances one owned reset clock at its existing scheduler deadline.
    Tick(NodeId),
    /// This publishes one deferred actor transition with an optional owned payload witness.
    Publish {
        actor: PistonId,
        extending: bool,
        resource: Option<(ResourceId, ContactId)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicRegionWrite {
    /// This identifies the sparse owned scalar slot changed by the event entry.
    pub(crate) slot: RegionStateSlot,
    /// This selects the new scalar value from the immutable Plan roots.
    pub(crate) root: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SymbolicRegionEffect {
    /// This schedules one node through the existing global half-tick queues.
    Schedule {
        when: usize,
        node: NodeId,
        delay: usize,
        priority: mchprs_world::TickPriority,
    },
    /// This appends one actuator request to the existing deduplicated FIFO.
    Request {
        when: usize,
        actor: PistonId,
        action: mchprs_world::PistonAction,
    },
    /// This appends one deferred publication with its event-time revision witnesses.
    Publication {
        when: usize,
        actor_valid: usize,
        actor: PistonId,
        actor_stamp: u32,
        resource: Option<(usize, ResourceId, u32, ContactId)>,
        due_tick: u64,
        extending: bool,
    },
    /// This records one accepted action using the ordinary presentation and movement hooks.
    Accepted {
        when: usize,
        actor: PistonId,
        action: mchprs_world::PistonAction,
        previous_extended: usize,
    },
    /// This invokes one ordinary outside callback with its event-time owned-state view.
    Callback {
        when: usize,
        occurrence: Option<RouteOccurrence>,
        target: SiteId,
        receiver: CallbackReceiver,
        kind: DeliveryKind,
        snapshot: Box<[SymbolicRegionWrite]>,
        captured: Option<SymbolicRegionCapture>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicRegionCapture {
    /// These alternatives retain the captured profile/receiver and event-time properties.
    pub(crate) variants: Box<[SymbolicRegionCaptureVariant]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicRegionCaptureVariant {
    /// This selects this exact captured contact alternative.
    pub(crate) when: usize,
    /// This is the originally observed profile, when the site is a contact.
    pub(crate) profile: Option<ProfileId>,
    /// This preserves the captured block material and static orientation.
    pub(crate) block: Block,
    /// This retains the graph's captured solidity fact.
    pub(crate) solid: bool,
    /// This retains the graph's captured transparency fact.
    pub(crate) transparent: bool,
    /// This is the captured callback receiver, if the site selected one.
    pub(crate) receiver: Option<CallbackReceiver>,
    /// This constructs the exact properties ordinary handlers observe at callback time.
    pub(crate) properties: SymbolicRegionCapturedProperties,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SymbolicRegionCapturedProperties {
    /// This block carried no callback properties.
    Passive,
    /// These are the captured powered and lock bits of an electrical block.
    Electrical { powered: usize, locked: usize },
    /// These are the captured powered and lit bits of a copper bulb.
    CopperBulb { powered: usize, lit: usize },
    /// These are the captured wire strength and four up/flat side pairs.
    Wire { signal: [usize; 4], sides: [(usize, usize); 4] },
    /// This is the captured piston extension bit.
    Piston { extended: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicRegionEventRoutine {
    /// This identifies the callback/sample/action/reset entry lowered by this plan.
    pub(crate) key: SymbolicRegionEntry,
    /// These are only live runtime values retained by the Plan input interface.
    pub(crate) reads: Box<[RegionRead]>,
    /// This executes the complete owned propagation closure and returns its live effects.
    pub(crate) plan: Plan,
    /// These are the only actor revision owners referenced by writes or effects.
    pub(crate) actor_owners: Box<[PistonId]>,
    /// These are the only payload revision owners referenced by writes or effects.
    pub(crate) resource_owners: Box<[ResourceId]>,
    /// These are sparse final state writes applied before event-stream replay.
    pub(crate) writes: Box<[SymbolicRegionWrite]>,
    /// These effects preserve the semantic order across scheduler, request and callback types.
    pub(crate) effects: Box<[SymbolicRegionEffect]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolicEventKernel {
    /// This identifies the original ownership region served by these entry plans.
    pub(crate) region: usize,
    /// These are shared entry routines, deduplicated by semantic payload schema.
    pub(crate) entries: Box<[std::sync::Arc<SymbolicRegionEventRoutine>]>,
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
    /// This immutable event kernel replaces all compiled internal callback closure execution.
    pub(crate) event_kernel: Option<std::sync::Arc<SymbolicEventKernel>>,
}

impl SymbolicRegionProgram {
    pub(crate) fn estimated_heap_bytes(&self) -> usize {
        self.actors.len() * std::mem::size_of::<std::sync::Arc<SymbolicActorRoutine>>()
            + self.resets.len() * std::mem::size_of::<std::sync::Arc<SymbolicResetProgram>>()
            + self.actors.iter().map(|actor| actor.estimated_heap_bytes()).sum::<usize>()
            + self.resets.iter().map(|reset| reset.estimated_heap_bytes()).sum::<usize>()
            + self
                .event_kernel
                .as_ref()
                .map_or(0, |kernel| kernel.entries.iter().map(|entry| entry.plan.estimated_heap_bytes()).sum())
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
    pub(crate) reason: String,
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
            + self
                .event_kernel
                .as_ref()
                .map_or(0, |kernel| kernel.entries.iter().map(|entry| entry.plan.estimated_heap_bytes()).sum())
    }
}

fn routine_bytes(routine: &EventRoutine) -> usize {
    routine.used_reads.len() * std::mem::size_of::<usize>() + routine.plan.estimated_heap_bytes()
}
