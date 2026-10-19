//! Graph-derived local equations for a stationary pull-computation wave.
//!
//! These are compile-time contracts, not executable kernels or a lifecycle certificate.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CellPinId(
    /// This indexes one earlier pin in the containing cell's equation table.
    pub(crate) u32,
);

impl CellPinId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellGateKind {
    /// This forwards the value of one operand.
    Buffer,
    /// This complements one value without generating an activation event.
    Invert,
    /// This requires every operand; the empty conjunction is true.
    All,
    /// This accepts any operand; the empty disjunction is false.
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellRead {
    /// This reads a guarded directional contribution, positive only above its attenuation.
    Contribution(PowerLink),
    /// This means a qualifying delivered event sampled this actor, independently of its power.
    Activation(PistonId),
    /// This reads existing local actor/resource state, never a copied wire-state input.
    State(StateTest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellPin {
    /// This preserves one typed value/event/state dependency from the prepared graph.
    Read(CellRead),
    Gate {
        /// This selects the value operation derived from the local physical rule.
        kind: CellGateKind,
        /// These earlier pins are the operation's operands in the same cell.
        inputs: Box<[CellPinId]>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellUse {
    /// This actor contributes one pull-computation use under the stationary entry contract.
    pub(crate) actor: PistonId,
    /// This records the existing reset source without inventing a clock for payload feedback.
    pub(crate) reset: CellReset,
    /// This combines Main contributions before Quasi, preserving their typed source ports.
    pub(crate) powered: CellPinId,
    /// This combines activation, an available extended base and absent sampled power.
    pub(crate) requested: CellPinId,
    /// This rechecks power, pose and payload destinations against preceding accepted writes.
    pub(crate) valid: CellPinId,
    /// These original callback occurrences can qualify a sample; guards and views stay in the graph.
    pub(crate) activations: Box<[RouteOccurrence]>,
    /// This preserves the accepted retraction, token transfer and existing reset/publication work.
    pub(crate) retraction: CellRetractionEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellTokenSelection {
    /// The first accepted owner request that still selects the token in Graph FIFO order transfers it.
    FirstAcceptedPullRequestInFifo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellResourcePin {
    /// This is the one conserved payload; shared actors do not receive separate tokens.
    pub(crate) resource: ResourceId,
    /// This is the common stationary source contact from which admitted actors pull it.
    pub(crate) source: ContactId,
    /// This is true only while the resource is enabled at its source contact.
    pub(crate) present: CellPinId,
    /// This derives disappearance from selected contact and electrical availability.
    pub(crate) absent: CellPinId,
    /// These actors can pull this token; list order does not choose the winner.
    pub(crate) owners: Box<[PistonId]>,
    /// This preserves the ordinary request queue's first-eligible-owner arbitration.
    pub(crate) selection: CellTokenSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellResourceTransfer {
    /// This is the conserved token selected for a possible accepted pull.
    pub(crate) resource: ResourceId,
    /// This is the token's shared Far source before the first owner takes it.
    pub(crate) source: ContactId,
    /// This is the accepting actor's Near destination.
    pub(crate) destination: ContactId,
    /// The token revision increments once when it is disabled at Near before Far/Near routes run.
    pub(crate) resource_revision_increment: u8,
    /// This snapshots the enabled token before the base-change callbacks run.
    pub(crate) when_at_source: CellPinId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellRetractionRoutes {
    /// This route runs first while the actor is BasePendingHeadPresent and the token is unmoved.
    pub(crate) base_changed: RouteId,
    /// This route runs after a selected token has moved to Near and become disabled.
    pub(crate) far_changed: Option<RouteId>,
    /// This route runs last in the synchronous accepted-retraction sequence.
    pub(crate) near_changed: RouteId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellResourceWake {
    /// This is restored independently of the accepting actor's revision.
    pub(crate) resource: ResourceId,
    /// This payload must still be disabled at this Near contact when publication arrives.
    pub(crate) contact: ContactId,
    /// This original route runs only after the resource revision/contact witness is valid.
    pub(crate) route: RouteId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellDeferredWake {
    /// Graph queues this accepted stationary retraction for logical_tick + 2.
    pub(crate) ticks_after_acceptance: u8,
    /// This original base route runs if the deferred event's captured actor revision still matches.
    pub(crate) base_restored: RouteId,
    /// This token publication has its own captured resource revision and may be stale independently.
    pub(crate) resource: Option<CellResourceWake>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellResetWake {
    /// This is the actor whose original base transitions the recognized observer watches.
    pub(crate) watched_actor: PistonId,
    /// These exact base-change rows may arm the existing reset node during acceptance.
    pub(crate) on_base_changed: Box<[RouteOccurrence]>,
    /// These exact base-restoration rows may arm it when the deferred event publishes.
    pub(crate) on_base_restored: Box<[RouteOccurrence]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellReset {
    Observer {
        /// This is the existing observer whose power returns to the actor.
        node: NodeId,
        /// These original observer deliveries preserve its scheduled reset work.
        wake: CellResetWake,
    },
    Payload {
        /// This actor's own restored Near payload supplies its reset power.
        resource: ResourceId,
        /// This original restoration callback starts a qualifying reset recheck.
        notification: RouteOccurrence,
        /// This original receiver samples the returned power after restoration.
        sample: RouteOccurrence,
    },
    InputRestoration {
        /// This already resettable upstream token supplies a restored input signal.
        resource: ResourceId,
        /// This is the existing Near or Far contact whose restoration supplies power.
        contact: ContactId,
        /// This original source delivery starts the qualifying reset notification path.
        notification: RouteOccurrence,
        /// This original actor delivery samples the restored power independently of its value.
        sample: RouteOccurrence,
    },
}

impl CellReset {
    pub(crate) fn observer(&self) -> Option<NodeId> {
        match self {
            Self::Observer { node, .. } => Some(*node),
            Self::Payload { .. } | Self::InputRestoration { .. } => None,
        }
    }

    pub(crate) fn observer_wake(&self) -> Option<&CellResetWake> {
        match self {
            Self::Observer { wake, .. } => Some(wake),
            Self::Payload { .. } | Self::InputRestoration { .. } => None,
        }
    }
}

/// This describes one pull-capable stationary retraction without replacing Graph's action scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellRetractionEffect {
    /// Graph increments and captures this actor revision before the first base-change callback.
    pub(crate) actor_revision_increment: u8,
    /// This transfer is conditional; an accepted owner with no token at Far still retracts.
    pub(crate) transfer: Option<CellResourceTransfer>,
    /// These retain base, conditional Far and final Near route order from Graph.
    pub(crate) routes: CellRetractionRoutes,
    /// This preserves the exact ordinary scheduler publication and its revision checks.
    pub(crate) wake: CellDeferredWake,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellContract {
    /// These graph-derived uses retain separate requests and acceptance predicates.
    pub(crate) uses: Box<[CellUse]>,
    /// These topological local equations derive values from typed reads and simple gates.
    pub(crate) pins: Box<[CellPin]>,
    /// This requires all bases extended/ready and all payloads stationary at their common Far.
    pub(crate) stationary_entry: CellPinId,
    /// These source-absence outputs can be connected to other cells without copying payloads.
    pub(crate) resources: Box<[CellResourcePin]>,
}

impl CellContract {
    /// Evaluate local equations in tests/proofs; production lowering substitutes their typed reads.
    #[cfg(test)]
    pub(crate) fn evaluate(&self, values: &mut Vec<bool>, mut read: impl FnMut(CellRead) -> bool) -> Result<(), &'static str> {
        values.clear();
        for pin in &self.pins {
            let value = match pin {
                CellPin::Read(source) => read(*source),
                CellPin::Gate { kind, inputs } => {
                    if inputs.iter().any(|input| input.index() >= values.len()) {
                        return Err("cell pin references a missing or later operand");
                    }
                    match kind {
                        CellGateKind::Buffer if inputs.len() == 1 => values[inputs[0].index()],
                        CellGateKind::Invert if inputs.len() == 1 => !values[inputs[0].index()],
                        CellGateKind::All => inputs.iter().all(|input| values[input.index()]),
                        CellGateKind::Any => inputs.iter().any(|input| values[input.index()]),
                        _ => return Err("unary cell gate has the wrong arity"),
                    }
                }
            };
            values.push(value);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellDecline {
    /// These actors belong to the indivisible group which was not certified.
    pub(crate) actors: Vec<PistonId>,
    /// This identifies the unsupported owner, material or source-contact rule.
    pub(crate) reason: CellDeclineReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellDeclineReason {
    /// This actor has no recognized reset provenance and may be ordinary memory.
    UnrecognizedActor(PistonId),
    /// This resource has different Far contacts among its owners, requiring another local contract.
    DifferentPullSources(ResourceId),
    /// This resource intersects the group but has no actor with a complete Near/Far pull lane.
    NoPullSource(ResourceId),
    /// This actor's accepted retract has no original base or payload restoration route.
    MissingDeferredWake(PistonId),
    /// Retaining this additional cell would exceed the graph-preparation allocation share.
    PinTableBudgetExceeded,
    /// This contact would need to contain two different stationary payloads at entry.
    ConflictingSourceContact(ContactId),
    /// This resource is not an admitted entity-free cube/redstone source.
    UnsupportedMaterial(ResourceId),
}
