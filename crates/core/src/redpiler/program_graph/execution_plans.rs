//! Derived compile-time plans. Circuit and resource state keep their original owners.
use super::*;

#[derive(Clone, Copy, Debug)]
/// The actual product replaced by a builder, used for required dependency checks.
pub(crate) enum PlanChange {
    MixedClassic,
    FixedClassic,
    Boundaries,
    Layers,
    SettledCircuit,
    FixedSignals,
    InstantDrives,
    InstantTransitionPrograms,
    SignalChannels,
    StableInstantLogic,
    InstantValueRegions,
    ClassicNets,
    TurboCallbacks,
    DustPaths,
    WireQueries,
    WireShapes,
    WireCaptures,
    WireCaptureMasks,
    InstantEvents,
    CpuKernels,
    CpuPowerQueries,
    BudMemoryReads,
    InstantTransport,
    InstantWireNodes,
    WireCallbacks,
    WireHops,
    InstantResets,
    Regions,
    Proxies,
}

impl PlanChange {
    /// These replacements can change instant ownership or its source catalog.
    pub(crate) fn replaces_layer_analysis(self) -> bool {
        matches!(self, Self::Boundaries | Self::Layers | Self::ClassicNets | Self::Regions | Self::Proxies)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
/// Optional execution shortcuts for one wire's unchanged canonical bindings.
pub(crate) struct WireExecutionPlan {
    /// Mutable canonical sites retained after immutable passive capture elimination.
    pub(crate) mutable_captures: Option<u32>,
    /// Experimental closed single-wire signal transfer.
    pub(crate) instant_node: Option<InstantWireNode>,
    /// Exact fixed admission; dynamic slots keep their original guard search.
    pub(crate) callback_selection: Option<WireCallbackSelection>,
    /// Checked fixed geometry removes repeated property resolution without caching power.
    pub(crate) query_facts: Option<WireQueryFacts>,
    /// Proven raw shapes and regulated output bits across all contact variants; strength stays live.
    pub(crate) shape: Option<WireShape>,
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
    /// Pure fixed-face reads; this plan owns no propagation or mutable state.
    pub(crate) mixed_classic: MixedClassicProgram,
    /// Whole fixed-circuit executable tables; excludes other execution owners atomically.
    pub(crate) fixed_classic: FixedClassicPlans,
    /// Exact compiled electrical reads and the separate settled weighted topology witness.
    pub(crate) fixed_signals: FixedSignalPlans,
    /// Typed no-dust source expressions; unsupported wires retain general queries.
    pub(crate) instant_drives: InstantDriveProgram,
    /// Exact episode eligibility maps over certified ordered transport nets.
    pub(crate) instant_transition_programs: InstantTransitionPrograms,
    pub(crate) signal_channels: SignalChannels,
    pub(crate) stable_instant_logic: StableInstantLogic,
    /// Exact bounded electrical distance rows for transport-net value regions.
    pub(crate) instant_value_regions: InstantValueRegions,
    /// Logical output/state equations; qualified O6 devices also execute their logical timing kernel.
    pub(crate) settled_circuit: Option<SettledCircuit>,
    /// Whole private wire regions with exact ordered delayed-receiver programs.
    pub(crate) classic_nets: Vec<ClassicNetPlan>,
    /// Empty disables indexing; otherwise entries align with the wire table.
    pub(crate) turbo_callback_indices: Vec<Option<TurboCallbackIndex>>,
    /// Empty uses general capture; otherwise aligned with sites, shared by all walks.
    pub(crate) wire_captures: Vec<WireCapture>,
    /// Experimental FIFO cell kernels; empty retains the existing pull-wave lowering.
    pub(crate) instant_events: Vec<InstantKernel>,
    /// Flat pure predicates compiled from event kernels; no mutable values are duplicated.
    pub(crate) cpu_kernels: CpuKernelPlans,
    /// Live Main/Quasi predicates for exact O6 piston power queries.
    pub(crate) cpu_power_queries: CpuPowerQueryPlans,
    /// Exact private storage/read pairs; their original state and update work remain owned by Graph.
    pub(crate) bud_memory_reads: Vec<BudMemoryRead>,
    /// Closed wire corridors with dense local Turbo bindings and ordered boundary effects.
    pub(crate) instant_transport: InstantTransport,
    /// Local wire-power certificates, retaining the original ordered walks.
    pub(crate) dust_paths: Vec<DustPathPlan>,
    /// Ownership boundaries and their dependent instant programs.
    pub(crate) instant: InstantPlans,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Instant region ownership and private-cell proxy plans.
pub(crate) struct InstantPlans {
    /// These cold causal layers are lowered separately into certified pull-wave executables.
    pub(crate) layers: Option<LayeredInstantPlan>,
    /// This cold experimental catalog is the sole owner map when the boundary pipeline is selected.
    pub(crate) boundaries: Option<BoundaryCatalog>,
    pub(crate) regions: Vec<InstantRegion>,
    pub(crate) proxies: Vec<InstantProxy>,
}
