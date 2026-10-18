//! Immutable read-only catalog built before instant regions are composed.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct BoundaryCatalog {
    /// These graph-derived local contracts are the only actors admitted as stationary cells.
    pub(crate) cells: Box<[CellContract]>,
    /// These complete ownership groups remain outside the catalog with their decline witnesses.
    pub(crate) declines: Box<[CellDecline]>,
    /// These non-classic wire corridors remain separate from the cells they touch.
    pub(crate) corridors: Box<[WireCorridor]>,
    /// These directed references describe every classified read or effect crossing an owner edge.
    pub(crate) ports: Box<[BoundaryPort]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireCorridor {
    /// These connected non-classic wire nodes may carry power or captured notifications.
    pub(crate) wires: Box<[NodeId]>,
    /// These constant solid sites conduct between the corridor's wires.
    pub(crate) conductive_sites: Box<[SiteId]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BoundaryOwner {
    /// This identifies one cell contract without merging it with a connected neighbor.
    Cell(u32),
    /// This identifies one connected wire corridor while retaining its separate ownership.
    Corridor(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BoundaryDirection {
    /// This dependency reads an outside value, event, guard, or geometry fact.
    Input,
    /// This dependency exposes an owned change to an outside reader or receiver.
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BoundaryPort {
    /// This is the cell or corridor whose behavior depends on or produces the port.
    pub(crate) owner: BoundaryOwner,
    /// This separates values/events consumed by the owner from effects it publishes.
    pub(crate) direction: BoundaryDirection,
    /// This references the original typed graph row instead of copying executable semantics.
    pub(crate) kind: BoundaryPortKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BoundaryPortKind {
    /// This references the original electrical or quasi-connectivity edge by ordinal.
    Power { link: u32 },
    /// This references one ordered callback occurrence; its kind, guard, direction, and view remain in the route.
    Callback { occurrence: RouteOccurrence },
    /// This references one guarded wire-to-site relation by its original ordinal.
    WireRelation { relation: u32 },
    /// This identifies a comparator read or override branch from its original plan.
    Comparator {
        /// This indexes the original comparator sampling plan.
        plan: u32,
        /// This distinguishes the rear read from each override dependency.
        role: ComparatorPortRole,
    },
    /// This records a direct guard-site dependency without copying the guard expression.
    GuardSite {
        /// This identifies the leaf atom; enclosing graph rows retain its root condition.
        guard: GuardId,
        /// This is the site whose property the atom reads.
        site: SiteId,
        /// This preserves whether the property comes from live state or a capture.
        view: OperandView,
        /// This selects the exact property tested at that site.
        test: SiteTest,
    },
    /// This records a direct guard-state dependency without copying the guard expression.
    GuardState {
        /// This identifies the original state-test leaf in the guard table.
        guard: GuardId,
        /// This binds the actor or resource state read by that leaf.
        test: StateTest,
    },
    /// This preserves a possible profile and receiver for one contact alternative.
    ContactAlternative {
        /// This is the changing position read across the interface.
        contact: ContactId,
        /// This indexes one possible profile/receiver in that contact's original variants.
        ordinal: u32,
    },
    /// This preserves an observer's geometry read independently of its callback routes.
    ObserverWatch {
        /// This identifies the existing observer reading the changing geometry.
        observer: NodeId,
        /// This is the observer's original watched site.
        site: SiteId,
    },
    /// This retains a captured Turbo occurrence and the exact neighboring site it may read.
    CapturedWire {
        /// This wire owns the Turbo route that captured the neighbor.
        wire: NodeId,
        /// This retains the exact captured delivery, including its original guard.
        occurrence: RouteOccurrence,
        /// This identifies the captured neighboring site read by that delivery.
        neighbor: SiteId,
    },
    /// This retains a canonical live wire-neighborhood read even when it has no captured delivery row.
    WireNeighborhood {
        /// This wire reads the site's power or geometry through its canonical neighborhood.
        wire: NodeId,
        /// This identifies the original neighbor and thus its canonical direction.
        neighbor: SiteId,
    },
    /// This identifies a note-block support or obstruction read.
    FixedRead {
        /// This is the ordinary note block whose edge samples the site.
        node: NodeId,
        /// This distinguishes instrument support from sound obstruction.
        role: FixedReadRole,
        /// This is the compiled site sampled by that role.
        site: SiteId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ComparatorPortRole {
    /// This is the comparator's rear input site.
    Rear,
    /// This is a direct override supplied by a literal or live component output.
    DirectOverride,
    /// This is the live component output used by the optional far override.
    FarOverrideOutput,
    /// This is the optional far override's source site.
    FarOverrideSite,
    /// This is the guard controlling the far override's applicability.
    FarOverrideGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FixedReadRole {
    /// This is the support site read by a note block when activated.
    NoteSupport,
    /// This is the obstruction site read by a note block when activated.
    NoteObstruction,
}
