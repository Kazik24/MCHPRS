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
    /// This power-link ordinal supplies a region-owned component input.
    Power(u32),
    /// This route occurrence is an external callback that may activate owned work.
    Callback(RouteOccurrence),
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
}

impl KernelBoundary {
    /// Copy the validated ownership boundary into deterministic, typed kernel ports.
    pub(crate) fn from_region(region: &InstantRegion) -> Self {
        let mut inputs: Vec<_> = region.power_inputs.iter().copied().map(KernelInput::Power).collect();
        inputs.extend(region.incoming.iter().copied().map(KernelInput::Callback));
        inputs.sort_by_key(|input| match input {
            KernelInput::Power(link) => (0, *link, 0),
            KernelInput::Callback(event) => (1, event.route.index() as u32, event.ordinal),
        });

        let mut state = Vec::new();
        state.extend(region.resets.iter().copied().map(KernelStateSlot::Reset));
        state.extend(region.actors.iter().copied().map(KernelStateSlot::Piston));
        state.extend(region.resources.iter().copied().map(KernelStateSlot::Resource));
        state.extend(region.wires.iter().copied().map(KernelStateSlot::Wire));

        let mut outputs: Vec<_> = region.power_readers.iter().copied().map(KernelOutput::Power).collect();
        outputs.extend(region.outgoing.iter().copied().map(KernelOutput::Callback));
        outputs.extend(region.observers.iter().copied().map(KernelOutput::Observer));
        outputs.sort_by_key(|output| match output {
            KernelOutput::Power(link) => (0, *link, 0),
            KernelOutput::Callback(event) => (1, event.route.index() as u32, event.ordinal),
            KernelOutput::Observer(node) => (2, node.index() as u32, 0),
        });

        let mut projection_roots = region.readable_sites.clone();
        projection_roots.sort_unstable();
        projection_roots.dedup();

        Self {
            inputs,
            state,
            outputs,
            effects: region.outgoing.clone(),
            projection_roots,
        }
    }
}

#[cfg(test)]
mod kernel_boundary_tests {
    use super::*;

    #[test]
    fn boundary_ports_are_ordered_and_keep_duplicate_effects() {
        let event = RouteOccurrence { route: RouteId::new(4), ordinal: 2 };
        let region = InstantRegion {
            stable_inputs: true,
            wires: vec![NodeId::new(3)],
            actors: vec![PistonId::new(1)],
            resources: vec![ResourceId::new(0)],
            resets: vec![NodeId::new(2)],
            contacts: vec![],
            readable_sites: vec![SiteId::new(5), SiteId::new(1), SiteId::new(5)],
            power_inputs: vec![7, 1],
            power_readers: vec![9, 3],
            routes: vec![],
            incoming: vec![event],
            outgoing: vec![event, event],
            wire_readers: vec![],
            observers: vec![NodeId::new(8)],
        };

        let boundary = KernelBoundary::from_region(&region);
        assert_eq!(boundary.inputs, [KernelInput::Power(1), KernelInput::Power(7), KernelInput::Callback(event)]);
        assert_eq!(boundary.state, [
            KernelStateSlot::Reset(NodeId::new(2)),
            KernelStateSlot::Piston(PistonId::new(1)),
            KernelStateSlot::Resource(ResourceId::new(0)),
            KernelStateSlot::Wire(NodeId::new(3)),
        ]);
        assert_eq!(boundary.effects, [event, event]);
        assert_eq!(boundary.projection_roots, [SiteId::new(1), SiteId::new(5)]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
