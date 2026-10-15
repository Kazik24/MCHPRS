//! Explicit region boundaries and certified instructions; mutable state stays in CircuitState.
use super::*;

/// Stable graph-derived ports and owned state required by a symbolic kernel.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct KernelBoundary {
    /// These are external power reads and ordered callback entries used by the region.
    pub(crate) inputs: Vec<KernelInput>,
    /// These are mutable owners whose values can affect a later activation.
    pub(crate) state: Vec<KernelStateSlot>,
    /// These are electrical or callback observations delivered outside the region.
    pub(crate) outputs: Vec<KernelOutput>,
    /// These are ordered notifications that the kernel must deliver to outside owners.
    pub(crate) effects: Vec<RouteOccurrence>,
    /// These private sites and contacts are needed to reconstruct supported handoffs.
    pub(crate) projection_roots: Vec<SiteId>,
}

/// One external dependency entering the region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum KernelInput {
    /// This supplies source power after port filtering but before link attenuation.
    Power(u32),
    /// This route occurrence is an external callback that may activate owned work.
    /// The occurrence retains its original captured site operands and receiver ordering.
    Callback(RouteOccurrence),
    /// This graph guard reads one or more external operands at this exact use.
    Guard(KernelGuardRead),
    /// This owned wire reads one external neighbor while rebuilding its live shape.
    WireNeighbor {
        /// This identifies the owned wire whose geometry reads the neighbor.
        wire: NodeId,
        /// This is the wire's canonical neighbor ordinal.
        slot: u8,
        /// This is the external dynamic site read at that ordinal.
        site: SiteId,
    },
}

/// One graph occurrence whose guard reads a typed operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct KernelGuardRead {
    /// This identifies the exact link, delivery, contact variant, wire relation, or comparator override.
    pub(crate) owner: KernelGuardUse,
    /// This is the original graph guard evaluated at that occurrence.
    pub(crate) guard: GuardId,
}

/// One graph occurrence that evaluates a guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum KernelGuardUse {
    /// This is one directional power-link row.
    Power(u32),
    /// This is one ordered route delivery, retaining duplicate rows.
    Callback(RouteOccurrence),
    /// This is one contact variant at its original ordinal.
    ContactVariant {
        /// This identifies the contact whose occupied profile is tested.
        contact: ContactId,
        /// This is the original contact-variant ordinal.
        ordinal: u32,
    },
    /// This is one comparator far-override guard at its original plan row.
    ComparatorOverride(u32),
    /// This is one wire relation at its original ordinal.
    WireRelation(u32),
}

/// One persistent mutable owner retained across activations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum KernelStateSlot {
    /// This node owns a reset clock and its pending scheduled work.
    Reset(NodeId),
    /// This actor owns piston pose and availability state.
    Piston(PistonId),
    /// This identity owns conserved shared payload location and enable state.
    Resource(ResourceId),
    /// This wire owns stored signal strength and shape state.
    Wire(NodeId),
}

/// One value or notification visible beyond the region boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum KernelOutput {
    /// This power-link ordinal exposes an owned site to an outside electrical reader.
    Power(u32),
    /// This route occurrence notifies an outside callback receiver.
    Callback(RouteOccurrence),
    /// This site is sampled by an observer outside the region.
    Observer(NodeId),
    /// This external wire reads one owned site while rebuilding its live shape.
    WireReader {
        /// This identifies the outside wire whose geometry reads the site.
        wire: NodeId,
        /// This is the wire's canonical neighbor ordinal.
        slot: u8,
        /// This is the region-owned dynamic site read at that ordinal.
        site: SiteId,
    },
    /// This external guard use reads an operand owned by the region.
    Guard(KernelGuardRead),
    /// This outside callback reads an owned delivery target using the original view.
    CallbackSite {
        /// This identifies the exact external delivery and its ordinal.
        event: RouteOccurrence,
        /// This is the graph target read by that delivery.
        site: SiteId,
        /// This preserves whether the callback reads a live site or event capture.
        view: OperandView,
    },
}

impl KernelBoundary {
    /// Derive typed ports; the graph must have passed full graph validation first.
    pub(crate) fn from_region(graph: &CompileGraph, region: &InstantRegion) -> Self {
        crate::redpiler::analysis::instant_regions::boundary::derive(graph, region)
    }

    /// Check this boundary against a graph and region that passed graph validation.
    pub(crate) fn validate(&self, graph: &CompileGraph, region: &InstantRegion) -> Result<(), &'static str> {
        crate::redpiler::analysis::instant_regions::boundary::validate(self, graph, region)
    }
}

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
