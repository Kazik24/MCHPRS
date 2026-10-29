//! Derived compile-time plans. Circuit and resource state keep their original owners.
use super::*;

#[derive(Clone, Copy, Debug)]
/// The actual product replaced by a builder, used for required dependency checks.
pub(crate) enum PlanChange {
    MixedClassic,
    FixedClassic,
    FixedSignals,
    ClassicNets,
    TurboCallbacks,
    DustPaths,
    WireQueries,
    WireShapes,
    WireCaptures,
    WireCallbacks,
    WireHops,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
/// Optional execution shortcuts for one wire's unchanged canonical bindings.
pub(crate) struct WireExecutionPlan {
    /// Exact fixed admission; dynamic slots keep their original guard search.
    pub(crate) callback_selection: Option<WireCallbackSelection>,
    /// Checked fixed geometry removes repeated property resolution without caching power.
    pub(crate) query_facts: Option<WireQueryFacts>,
    /// Proven raw shapes and regulated output bits across all contact variants; strength stays live.
    pub(crate) shape: Option<WireShape>,
    /// This binds the root for compiled hops; neighbors reuse canonical_neighbors.
    pub(crate) hop_root: Option<SiteId>,
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
    /// Pure fixed-face reads; this plan owns no propagation or mutable state.
    pub(crate) mixed_classic: MixedClassicProgram,
    /// Whole fixed-circuit executable tables; excludes other execution owners atomically.
    pub(crate) fixed_classic: FixedClassicPlans,
    /// Exact compiled electrical reads and the separate settled weighted topology witness.
    pub(crate) fixed_signals: FixedSignalPlans,
    /// Whole private wire regions with exact ordered delayed-receiver programs.
    pub(crate) classic_nets: Vec<ClassicNetPlan>,
    /// Empty disables indexing; otherwise entries align with the wire table.
    pub(crate) turbo_callback_indices: Vec<Option<TurboCallbackIndex>>,
    /// Empty uses general capture; otherwise aligned with sites, shared by all walks.
    pub(crate) wire_captures: Vec<WireCapture>,
    /// Local wire-power certificates, retaining the original ordered walks.
    pub(crate) dust_paths: Vec<DustPathPlan>,
}
