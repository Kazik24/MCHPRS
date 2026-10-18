//! Derived compile-time plans. Circuit and resource state keep their original owners.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
/// Optional execution shortcuts for one wire's unchanged canonical bindings.
pub(crate) struct WireExecutionPlan {
    /// Checked fixed geometry removes repeated property resolution without caching power.
    pub(crate) query_facts: Option<WireQueryFacts>,
    /// This replaces a certified private reset cell's output walk with its exact two-wire transfer.
    pub(crate) instant_reset: Option<InstantResetPlan>,
    /// This binds the root for compiled hops; neighbors reuse canonical_neighbors.
    pub(crate) hop_root: Option<SiteId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Bindings for the first private redstone-resource/base-observer reset family.
pub(crate) struct InstantResetPlan {
    /// This identifies the existing actor whose lane and callbacks close the mechanism.
    pub(crate) piston: PistonId,
    /// This identifies the sole payload owner; no proxy copies its location or revision.
    pub(crate) resource: ResourceId,
    /// This identifies the base-watching reset observer and its existing scheduled state.
    pub(crate) observer: NodeId,
    /// This identifies the second wire in the certified two-wire output line.
    pub(crate) partner: NodeId,
}

/// A certified short dust line whose only variable input is the first wire's direct source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DustPathPlan {
    /// This identifies the component whose output is the path's one external input.
    pub(crate) input_node: NodeId,
    /// This is the compiled site sampled for the source's directional output.
    pub(crate) input_site: SiteId,
    /// This preserves the receiver-to-source face used by the ordinary wire query.
    pub(crate) input_face: BlockFace,
    /// These are the original wire state owners, ordered from ingress to the free end.
    pub(crate) wires: Box<[NodeId]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Delimits each canonical target's ordered slice in one wire's Turbo route.
pub(crate) struct TurboCallbackIndex {
    pub(crate) offsets: [u32; 25],
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Derived execution data. A graph rewrite discards it; builders publish typed outputs.
pub(crate) struct ExecutionPlans {
    /// Whole private wire regions with exact ordered delayed-receiver programs.
    pub(crate) classic_nets: Vec<ClassicNetPlan>,
    /// Empty disables indexing; otherwise entries align with the wire table.
    pub(crate) turbo_callback_indices: Vec<Option<TurboCallbackIndex>>,
    /// Local wire-power certificates, retaining the original ordered walks.
    pub(crate) dust_paths: Vec<DustPathPlan>,
    /// Ownership boundaries and their dependent instant programs.
    pub(crate) instant: InstantPlans,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Instant region ownership and private-cell proxy plans.
pub(crate) struct InstantPlans {
    pub(crate) regions: Vec<InstantRegion>,
    pub(crate) proxies: Vec<InstantProxy>,
}
