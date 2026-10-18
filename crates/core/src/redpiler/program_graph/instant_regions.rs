//! Explicit region boundaries and certified instructions; mutable state stays in CircuitState.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RouteOccurrence {
    /// This identifies the original ordered route containing the boundary event.
    pub(crate) route: RouteId,
    /// This is the delivery's ordinal, retaining duplicates independently of sequence values.
    pub(crate) ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantRegion {
    /// This declares stable external inputs from an activating falling edge until the composed cycle resets.
    pub(crate) stable_inputs: bool,
    /// These wires are internal to a composed region; their ordered execution is retained.
    pub(crate) wires: Vec<NodeId>,
    /// These actors share ownership or an explicitly composed stable-input cycle boundary.
    pub(crate) actors: Vec<PistonId>,
    /// These are conserved payload identities reachable by the region's actors.
    pub(crate) resources: Vec<ResourceId>,
    /// These original component IDs own the region's reset clocks and pending ticks.
    pub(crate) resets: Vec<NodeId>,
    /// These contacts include every possible base, head, and payload position.
    pub(crate) contacts: Vec<ContactId>,
    /// These sites expose all owned geometry and component state to external queries.
    pub(crate) readable_sites: Vec<SiteId>,
    /// These original power-link ordinals supply component input queries owned by the region.
    pub(crate) power_inputs: Vec<u32>,
    /// These original power-link ordinals expose owned sites to external power queries.
    pub(crate) power_readers: Vec<u32>,
    /// These routes originate in owned actors, contacts, reset clocks, or internal wires.
    pub(crate) routes: Vec<RouteId>,
    /// These ordered occurrences enter from another owner, retaining direction, guard, and capture in the original row.
    pub(crate) incoming: Vec<RouteOccurrence>,
    /// These ordered occurrences leave the region and cannot be absorbed by its proxy.
    pub(crate) outgoing: Vec<RouteOccurrence>,
    /// These external wires can capture or query an owned site in a Turbo walk.
    pub(crate) wire_readers: Vec<NodeId>,
    /// These external observers can watch an owned site's intermediate changes.
    pub(crate) observers: Vec<NodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstantStepKind {
    /// Dispatch preserves the original externally visible callback handler.
    Dispatch,
    /// Sample directly invokes the owning actor at the original notification position.
    Sample(PistonId),
    /// ArmReset directly schedules the existing reset clock with its original deadline and deduplication.
    ArmReset(NodeId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InstantStep {
    /// This retains the complete original event as an independently validated witness.
    pub(crate) delivery: RouteDelivery,
    /// This selects a certified primitive without changing order or reentrant execution.
    pub(crate) kind: InstantStepKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstantRoute {
    /// This identifies the original route replaced by the instruction sequence.
    pub(crate) route: RouteId,
    /// These instructions preserve every original row in order, including meaningful duplicates.
    pub(crate) steps: Box<[InstantStep]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstantProxy {
    /// This binds the proxy to its complete compile-time ownership boundary.
    pub(crate) region: usize,
    /// These independently certified cells retain the private-resource occupancy fast path.
    pub(crate) private_cells: Box<[PrivateInstantCell]>,
    /// These compiled routes absorb internal dispatch while preserving outward deliveries.
    pub(crate) routes: Box<[InstantRoute]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrivateInstantCell {
    /// This actor is the only base or head that can occupy the payload's lane.
    pub(crate) actor: PistonId,
    /// This conserved payload is the only resource allowed in that lane.
    pub(crate) resource: ResourceId,
    /// This retains the original self-reset clock and pending obligations.
    pub(crate) reset: NodeId,
}
