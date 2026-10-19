//! Typed construction and immutable program storage for the general graph.

mod execution_plans;
mod classic_redstone;
pub(crate) use classic_redstone::*;
mod lowering;
mod validation;
pub(crate) use execution_plans::*;
mod instant_regions;
mod instant_cells;
mod instant_boundaries;
mod instant_layers;
mod instant_program;
mod wire_query_facts;
mod wire_callback_selection;
pub(crate) use wire_callback_selection::WireCallbackSelection;
mod fixed_signals;
pub(crate) use fixed_signals::*;
pub(crate) use instant_regions::*;
pub(crate) use instant_cells::*;
pub(crate) use instant_boundaries::*;
mod fixed_classic;
pub(crate) use fixed_classic::*;
mod mixed_classic;
pub(crate) use mixed_classic::*;
pub(crate) use instant_layers::*;
pub(crate) use instant_program::*;
pub(crate) use wire_query_facts::WireQueryFacts;

use crate::redpiler::components::{NodeState, NodeType};
use mchprs_blocks::blocks::{Block, RedstoneWire};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{TickEntry, TickPriority};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;

macro_rules! graph_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub(crate) struct $name(u32);

        impl $name {
            pub(crate) const fn new(index: u32) -> Self {
                Self(index)
            }

            pub(crate) fn try_from_index(index: usize) -> Option<Self> {
                u32::try_from(index).ok().map(Self::new)
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

graph_id!(NodeId);
graph_id!(SiteId);
graph_id!(PistonId);
graph_id!(ResourceId);
graph_id!(ContactId);
graph_id!(ProfileId);
graph_id!(GuardId);
graph_id!(RouteId);
graph_id!(DustPathId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Which component input receives an electrical dependency.
pub(crate) enum InputPort {
    Main,
    Side,
    Quasi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Which kind of power a compiled site exposes to a query.
pub(crate) enum PowerPort {
    Output,
    Stored,
    Weak(BlockFace),
    Strong(BlockFace),
    Conducted { include_dust: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Whether a callback link represents a block update or shape notification.
pub(crate) enum LinkKind {
    Update { direction: Option<BlockFace>, guard: GuardId },
    Shape { direction: BlockFace, guard: GuardId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One callback dependency between compiled nodes.
pub(crate) struct LinkSpec {
    pub(crate) from: NodeId,
    pub(crate) to: NodeId,
    pub(crate) kind: LinkKind,
    pub(crate) callback: CallbackEdgeRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Identifies one ordered delivery within a prepared callback route.
pub(crate) struct CallbackEdgeRef {
    pub(crate) route: RouteId,
    pub(crate) sequence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Runtime category and state owner for a node in the circuit draft.
pub(crate) enum CircuitNodeKind {
    Electrical(NodeType),
    Observer {
        watched: SiteId,
        facing: BlockFace,
    },
    /// A reset pulse source executed directly by the scheduler, without an output callback circuit.
    BuiltinReset {
        /// This retains the original watched site for provenance and projection.
        watched: SiteId,
        /// This preserves which directional notifications arm the shared reset clock.
        facing: BlockFace,
    },
    Wire(usize),
    Piston(PistonId),
    Contact(ContactId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Initial mutable state representation for a circuit node.
pub(crate) enum ComponentState {
    Electrical(NodeState),
    Wire(RedstoneWire),
    Specialized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Mutable node data collected while building the circuit draft.
pub(crate) struct CircuitNodeDraft {
    pub(crate) position: Option<BlockPos>,
    /// Original registry state; runtime uses it for static properties and tick type.
    pub(crate) raw_state: Option<u32>,
    pub(crate) kind: CircuitNodeKind,
    pub(crate) state: ComponentState,
    pub(crate) output_route: Option<RouteId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Static node data used to create the immutable execution program.
pub(crate) struct NodeSpec {
    pub(crate) position: Option<BlockPos>,
    pub(crate) raw_state: Option<u32>,
    pub(crate) block: Option<Block>,
    pub(crate) kind: CircuitNodeKind,
    pub(crate) output_route: Option<RouteId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Compiled predecessor and successor relationships for a command block.
pub(crate) struct CommandBlockPlan {
    /// This identifies the command block whose state is stored in `CircuitState`.
    pub(crate) node: NodeId,
    /// This binds conditional execution to its compiled predecessor, when present.
    pub(crate) previous: Option<NodeId>,
    /// This binds chain traversal to the next compiled chain command, when present.
    pub(crate) next: Option<NodeId>,
    /// This records whether activation and execution read predecessor success.
    pub(crate) conditional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Compiled sites sampled when a note block is activated.
pub(crate) struct NoteBlockPlan {
    /// This identifies the noteblock whose powered and instrument state is live.
    pub(crate) node: NodeId,
    /// This is the compiled support block sampled for its instrument.
    pub(crate) below: SiteId,
    /// This is the compiled obstruction block sampled for sound eligibility.
    pub(crate) above: SiteId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Electrical role of a block profile.
pub(crate) enum ProfileElectrical {
    None,
    RedstoneSource,
    Component,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Static block properties shared by compiled sites.
pub(crate) struct Profile {
    pub(crate) raw_state: u32,
    pub(crate) registry_type: u32,
    pub(crate) solid: bool,
    pub(crate) transparent: bool,
    pub(crate) cube: bool,
    pub(crate) electrical: ProfileElectrical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Selects whether a guard reads live state or a captured delivery snapshot.
pub(crate) enum OperandView {
    Live,
    Captured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// A state predicate evaluated by a graph guard.
pub(crate) enum StateTest {
    PistonExtended(PistonId),
    PistonHeadPresent(PistonId),
    PistonAvailability { piston: PistonId, is: PistonAvailability },
    ResourceAt { resource: ResourceId, contact: ContactId },
    ResourceEnabled(ResourceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// A static or captured block-site predicate evaluated by a graph guard.
pub(crate) enum SiteTest {
    Solid,
    Transparent,
    IsWire,
    Profile(ProfileId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Boolean condition that controls a compiled callback or power relation.
pub(crate) enum Guard {
    Always,
    State(StateTest),
    Site { site: SiteId, view: OperandView, test: SiteTest },
    Not(GuardId),
    All(Box<[GuardId]>),
    Any(Box<[GuardId]>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// The kind of compiled receiver notified by a callback delivery.
pub(crate) enum CallbackReceiver {
    Component(NodeId),
    PistonBase(PistonId),
    PistonHead(PistonId),
    Contact(ContactId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One guarded contact profile and its optional callback receiver.
pub(crate) struct ContactVariant {
    pub(crate) when: GuardId,
    pub(crate) profile: ProfileId,
    pub(crate) receiver: Option<CallbackReceiver>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Profile variants that can occupy a physical contact position.
pub(crate) struct ContactSpec {
    pub(crate) node: NodeId,
    pub(crate) position: BlockPos,
    pub(crate) variants: Box<[ContactVariant]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Material profile and contacts that may contain it.
pub(crate) struct ResourceSpec {
    pub(crate) material: ProfileId,
    pub(crate) contacts: Box<[ContactId]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Current location and activation revision of a movable resource.
pub(crate) struct ResourceState {
    pub(crate) at: ContactId,
    pub(crate) enabled: bool,
    pub(crate) revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContactCandidates<T> {
    /// This maps each dense contact ID to its slice in `ids`.
    ranges: Box<[Range<u32>]>,
    /// This stores typed contact-local candidates contiguously.
    ids: Box<[T]>,
}

impl<T: Copy> ContactCandidates<T> {
    fn from_buckets(buckets: Vec<Vec<T>>, table: &'static str) -> Result<Self, GraphBuildError> {
        let mut ranges = Vec::with_capacity(buckets.len());
        let mut ids = Vec::new();
        for bucket in buckets {
            let start = u32::try_from(ids.len()).map_err(|_| GraphBuildError::TooManyEntries { table })?;
            ids.extend(bucket);
            let end = u32::try_from(ids.len()).map_err(|_| GraphBuildError::TooManyEntries { table })?;
            ranges.push(start..end);
        }
        Ok(Self {
            ranges: ranges.into_boxed_slice(),
            ids: ids.into_boxed_slice(),
        })
    }

    fn at(&self, contact: ContactId) -> &[T] {
        self.ranges
            .get(contact.index())
            .map(|range| &self.ids[range.start as usize..range.end as usize])
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Empty or single-resource geometry in front of a piston.
pub(crate) enum PistonLane {
    Empty,
    SingleResource { far: ContactId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Callback routes used during piston transitions and restoration.
pub(crate) struct PistonRoutes {
    pub(crate) base_changed: RouteId,
    pub(crate) near_changed: RouteId,
    pub(crate) far_changed: Option<RouteId>,
    pub(crate) far_shape_changed: Option<RouteId>,
    pub(crate) base_restored: RouteId,
    pub(crate) head_restored: RouteId,
    pub(crate) resource_restored_near: Option<RouteId>,
    pub(crate) resource_restored_far: Option<RouteId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Static geometry and callback bindings for a compiled piston.
pub(crate) struct PistonSpec {
    pub(crate) node: NodeId,
    /// This records the piston base location for graph inspection and diagnostics.
    pub(crate) position: BlockPos,
    pub(crate) facing: BlockFace,
    pub(crate) sticky: bool,
    pub(crate) base: ContactId,
    pub(crate) near: ContactId,
    /// This identifies the far-side contact even when the piston lane is empty.
    pub(crate) far: ContactId,
    pub(crate) lane: PistonLane,
    pub(crate) routes: PistonRoutes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Availability phase used to guard piston-dependent graph behavior.
pub(crate) enum PistonAvailability {
    Ready,
    HeadPending,
    PublishingHeadRemoval,
    BasePendingHeadPresent,
    BasePending,
}

impl PistonAvailability {
    pub(crate) fn base_pending(self) -> bool {
        matches!(self, Self::BasePendingHeadPresent | Self::BasePending)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Mutable extension and publication state of a compiled piston.
pub(crate) struct PistonState {
    pub(crate) extended: bool,
    /// Tracks head occupancy across the piston transition publication phases.
    pub(crate) head_present: bool,
    pub(crate) availability: PistonAvailability,
    pub(crate) revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Source from which a compiled site obtains its block properties.
pub(crate) enum SiteSource {
    Component(NodeId),
    Contact(ContactId),
    Constant(ProfileId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A sampled world position and the compiled source that represents it.
pub(crate) struct SiteSpec {
    pub(crate) position: BlockPos,
    pub(crate) source: SiteSource,
    pub(crate) source_strength: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Compiled neighbor sites and callback routes for one redstone wire.
pub(crate) struct WirePlan {
    pub(crate) node: NodeId,
    pub(crate) canonical_neighbors: [SiteId; 24],
    pub(crate) shape_route: Option<RouteId>,
    /// Canonical captured candidates are ordered by TURBO_ORDER at delivery.
    pub(crate) turbo_route: Option<RouteId>,
    /// Rebuilt after semantic rewrites; never a second owner of electrical state.
    pub(crate) execution: WireExecutionPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Callback delivery form for one update or shape notification.
pub(crate) enum DeliveryKind {
    Update { direction: Option<BlockFace> },
    Shape { direction: BlockFace },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One ordered, guarded notification from a compiled callback route.
pub(crate) struct RouteDelivery {
    pub(crate) target: SiteId,
    pub(crate) kind: DeliveryKind,
    pub(crate) guard: GuardId,
    pub(crate) view: OperandView,
    pub(crate) sequence: u32,
    pub(crate) receiver: Option<CallbackReceiver>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Ordered notifications emitted when a node changes.
pub(crate) struct Route {
    pub(crate) source: NodeId,
    pub(crate) deliveries: Box<[RouteDelivery]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Destination of an electrical power relation.
pub(crate) enum PowerTarget {
    Input { node: NodeId, port: InputPort },
    Conductor { site: SiteId, include_dust: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Directional power transfer between compiled sites or component inputs.
pub(crate) struct PowerLink {
    pub(crate) from: SiteId,
    pub(crate) from_port: PowerPort,
    pub(crate) to: PowerTarget,
    pub(crate) face: BlockFace,
    pub(crate) attenuation: u8,
    pub(crate) guard: GuardId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Comparator override from a fixed strength or live component output.
pub(crate) enum ComparatorOverride {
    Literal(u8),
    LiveComponentOutput(NodeId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Compiled rear input and override checks for a comparator.
pub(crate) struct ComparatorInputPlan {
    pub(crate) node: NodeId,
    pub(crate) rear: SiteId,
    pub(crate) direct_override: Option<ComparatorOverride>,
    pub(crate) far_override: Option<(SiteId, ComparatorOverride, GuardId)>,
    pub(crate) far_threshold: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Geometric relation between a wire and a neighboring query site.
pub(crate) enum WireRelationKind {
    /// This denotes a same-level neighbor dependency.
    Step,
    /// This denotes a wire dependency that climbs a block face.
    Ascend,
    /// This denotes a wire dependency that descends a block face.
    Descend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A guarded wire-to-site dependency discovered from wire geometry.
pub(crate) struct WireRelation {
    /// This identifies the wire that discovered the dependency.
    pub(crate) from: NodeId,
    /// This identifies the dependent query site.
    pub(crate) to: SiteId,
    /// This classifies the geometric relation between wire and site.
    pub(crate) kind: WireRelationKind,
    /// This preserves the literal direction used during discovery.
    pub(crate) direction: BlockFace,
    /// This guards the relation under the selected contact state.
    pub(crate) guard: GuardId,
}

impl RouteDelivery {
    pub(crate) fn link_kind(self) -> LinkKind {
        match self.kind {
            DeliveryKind::Update { direction } => LinkKind::Update {
                direction,
                guard: self.guard,
            },
            DeliveryKind::Shape { direction } => LinkKind::Shape {
                direction,
                guard: self.guard,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Mutable circuit description, entry state, and separately owned derived plans.
pub(crate) struct CircuitDraft {
    pub(crate) nodes: Vec<CircuitNodeDraft>,
    pub(crate) links: Vec<LinkSpec>,
    pub(crate) power_links: Vec<PowerLink>,
    pub(crate) comparator_inputs: Vec<ComparatorInputPlan>,
    pub(crate) profiles: Vec<Profile>,
    pub(crate) guards: Vec<Guard>,
    pub(crate) contacts: Vec<ContactSpec>,
    pub(crate) routes: Vec<Route>,
    pub(crate) wires: Vec<WirePlan>,
    pub(crate) wire_relations: Vec<WireRelation>,
    pub(crate) sites: Vec<SiteSpec>,
    pub(crate) pistons: Vec<PistonSpec>,
    pub(crate) resources: Vec<ResourceSpec>,
    pub(crate) initial_pistons: Vec<PistonState>,
    pub(crate) initial_resources: Vec<ResourceState>,
    pub(crate) plans: ExecutionPlans,
}

/// The complete mutable graph shared by preparation, transformations, and export.
pub(crate) type CompileGraph = CircuitDraft;

#[derive(Debug, Clone)]
/// Clock and pending scheduler entries captured at compilation time.
pub(crate) struct EntryState {
    /// This initializes the executor clock from the compilation boundary.
    pub(crate) logical_tick: u64,
    /// This retains imported work and entry target facts in original queue order.
    pub(crate) pending_ticks: Vec<EntryTick>,
}

#[derive(Debug, Clone)]
/// A pending tick plus the target type observed at the compilation boundary.
pub(crate) struct EntryTick {
    /// This preserves the scheduler's original target, block type, deadline, and priority.
    pub(crate) tick: TickEntry,
    /// This records a known loaded target type; None means its chunk was unavailable.
    pub(crate) current_registry_type: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
/// Scheduler entry translated for graph execution while preserving stale-tick checks.
pub(crate) struct ImportedTick {
    /// This binds the tick to a compiled receiver when one exists.
    pub(crate) target: Option<NodeId>,
    /// This preserves the original scheduled position for stale checks and handoff.
    pub(crate) position: BlockPos,
    /// This preserves the input scheduler's optional expected block type.
    pub(crate) block_type: Option<u32>,
    /// This retains the remaining half-tick deadline at compilation entry.
    pub(crate) delay: u32,
    /// This preserves the input scheduler's priority.
    pub(crate) priority: TickPriority,
    /// This records the block type that was current at the compilation boundary.
    pub(crate) current_registry_type: Option<u32>,
}

/// Immutable execution program and its initial state, clock, and imported work.
pub(crate) struct LoweredCircuit {
    /// This owns the immutable lowered instructions used by graph execution.
    pub(crate) program: CircuitProgram,
    /// This owns the initial mutable component, piston, and resource state.
    pub(crate) state: CircuitState,
    /// This initializes the graph clock from the entry boundary.
    pub(crate) logical_tick: u64,
    /// This transfers prepared ticks once into the graph scheduler.
    pub(crate) imported_ticks: Vec<ImportedTick>,
    /// This shares the lowering position index with graph controls and tick binding.
    pub(crate) positions: FxHashMap<BlockPos, NodeId>,
}

impl CircuitDraft {
    /// Called before any semantic rewrite, so no pass must maintain another pass's cache.
    pub(crate) fn clear_execution_plans(&mut self) {
        self.plans = ExecutionPlans::default();
        for wire in &mut self.wires {
            wire.execution = WireExecutionPlan::default();
        }
    }
}

pub(crate) fn command_facing(block: Block) -> BlockFace {
    match block.property("facing") {
        Some("up") => BlockFace::Top,
        Some("down") => BlockFace::Bottom,
        Some("east") => BlockFace::East,
        Some("west") => BlockFace::West,
        Some("south") => BlockFace::South,
        _ => BlockFace::North,
    }
}

#[derive(Debug)]
/// Structural validation failures encountered while building or lowering a graph.
pub(crate) enum GraphBuildError {
    InvalidFixedSignals,
    InvalidFixedClassic,
    InvalidMixedClassic,
    InvalidClassicNet { net: usize, reason: &'static str },
    InvalidWireCallbackSelection { wire: usize },
    InvalidInstantRegion {
        /// This names the derived table whose witness no longer agrees with the graph.
        table: &'static str,
        /// This identifies the first stale or missing certificate in that table.
        index: usize,
    },
    PreparationMemoryLimit {
        stage: &'static str,
        estimated_bytes: usize,
        limit_bytes: usize,
    },
    TooManyEntries {
        table: &'static str,
    },
    InvalidReference {
        table: &'static str,
        index: usize,
        len: usize,
    },
    InvalidStrength {
        node: NodeId,
        strength: u8,
    },
    InvalidPowerAttenuation {
        from: SiteId,
        attenuation: u8,
    },
    InvalidDustPath {
        path: usize,
        reason: &'static str,
    },
    InvalidTurboCallbackIndex {
        wire: usize,
        reason: &'static str,
    },
    InvalidWireHopRoot {
        wire: usize,
    },
    InvalidWireQueryFacts {
        wire: usize,
    },
    InvalidInstantReset {
        wire: usize,
    },
    InvalidNodeState {
        node: NodeId,
    },
    InvalidComponentBinding {
        node: NodeId,
    },
    MissingComponentState {
        node: NodeId,
    },
    UseSpecializedNode {
        node: NodeId,
    },
    NodeKindMismatch {
        node: NodeId,
    },
    CyclicGuard {
        guard: GuardId,
    },
    UnorderedRoute {
        route: RouteId,
    },
    RouteSourceMismatch {
        route: RouteId,
        node: NodeId,
    },
    CallbackEdgeMismatch {
        route: RouteId,
        sequence: u32,
    },
    PistonFarContactMismatch {
        node: NodeId,
    },
    PistonResourceRestorationMismatch {
        node: NodeId,
    },
    ContactGuardReadsSite {
        contact: ContactId,
        guard: GuardId,
        site: SiteId,
    },
    EmptyContact {
        contact: ContactId,
    },
    EmptyResource {
        resource: ResourceId,
    },
    InvalidResourceSelection {
        resource: ResourceId,
        contact: ContactId,
    },
    ResourcesShareContact {
        first: ResourceId,
        second: ResourceId,
        contact: ContactId,
    },
    StateTableLength {
        table: &'static str,
        descriptions: usize,
        states: usize,
    },
}

impl std::fmt::Display for GraphBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFixedSignals => write!(f, "fixed signal queries have a stale or invalid certificate"),
            Self::InvalidFixedClassic => write!(f, "whole fixed classic program has a stale or invalid certificate"),
            Self::InvalidMixedClassic => write!(f, "mixed classic query program has a stale or invalid certificate"),
            Self::PreparationMemoryLimit {
                stage,
                estimated_bytes,
                limit_bytes,
            } => write!(
                f,
                "Graph {stage} table estimate ({estimated_bytes} bytes) exceeds the {limit_bytes}-byte preparation limit"
            ),
            Self::TooManyEntries { table } => {
                write!(f, "{table} table exceeds the supported u32 index range")
            }
            Self::InvalidReference { table, index, len } => {
                write!(f, "{table} reference {index} is out of range 0..{len}")
            }
            Self::InvalidStrength { node, strength } => {
                write!(f, "node {} has strength {strength}; expected 0..15", node.index())
            }
            Self::InvalidPowerAttenuation { from, attenuation } => {
                write!(f, "power source site {} has attenuation {attenuation}; expected 0..15", from.index())
            }
            Self::InvalidDustPath { path, reason } => {
                write!(f, "dust path {path} is invalid: {reason}")
            }
            Self::InvalidTurboCallbackIndex { wire, reason } => {
                write!(f, "Turbo callback index for wire {wire} is invalid: {reason}")
            }
            Self::InvalidWireHopRoot { wire } => write!(f, "wire {wire} has an invalid compiled hop root"),
            Self::InvalidWireQueryFacts { wire } => write!(f, "wire {wire} has stale or invalid static query facts"),
            Self::InvalidWireCallbackSelection { wire } => write!(f,"wire {wire} has an invalid compiled callback selection"),
            Self::InvalidClassicNet { net,reason } => write!(f,"classic net {net} has an invalid certificate: {reason}"),
            Self::InvalidInstantRegion { table, index } => write!(f, "instant {table}[{index}] has a stale or invalid certificate"),
            Self::InvalidInstantReset { wire } => write!(f, "wire {wire} has a stale or invalid instant reset certificate"),
            Self::InvalidNodeState { node } => {
                write!(f, "node {} has state that does not match its kind", node.index())
            }
            Self::InvalidComponentBinding { node } => write!(f, "generic component binding references specialized node {}", node.index()),
            Self::MissingComponentState { node } => write!(f, "component node {} has no original block state", node.index()),
            Self::UseSpecializedNode { node } => write!(f, "node {} must use its typed wire or observer form", node.index()),
            Self::NodeKindMismatch { node } => write!(f, "description for node {} does not match its graph kind", node.index()),
            Self::CyclicGuard { guard } => {
                write!(f, "guard {} participates in a cycle", guard.index())
            }
            Self::UnorderedRoute { route } => write!(f, "route {} has deliveries out of sequence order", route.index()),
            Self::RouteSourceMismatch { route, node } => write!(f, "route {} is assigned to the wrong source node {}", route.index(), node.index()),
            Self::CallbackEdgeMismatch { route, sequence } => {
                write!(f, "route {} delivery {sequence} does not match its callback edge", route.index())
            }

            Self::PistonFarContactMismatch { node } => {
                write!(f, "piston node {} has conflicting far contacts in its description and lane", node.index())
            }
            Self::PistonResourceRestorationMismatch { node } => write!(
                f,
                "piston node {} has restoration routes that do not match its resource lane",
                node.index()
            ),
            Self::ContactGuardReadsSite { contact, guard, site } => write!(
                f,
                "contact {} selector guard {} reads site {}",
                contact.index(),
                guard.index(),
                site.index()
            ),
            Self::EmptyContact { contact } => {
                write!(f, "contact {} has no selectable variants", contact.index())
            }
            Self::EmptyResource { resource } => {
                write!(f, "resource {} has no possible contacts", resource.index())
            }
            Self::InvalidResourceSelection { resource, contact } => write!(
                f,
                "resource {} selects contact {} outside its description",
                resource.index(),
                contact.index()
            ),
            Self::ResourcesShareContact { first, second, contact } => write!(
                f,
                "resources {} and {} both claim contact {}",
                first.index(),
                second.index(),
                contact.index()
            ),
            Self::StateTableLength { table, descriptions, states } => {
                write!(f, "{table} table has {descriptions} descriptions but {states} initial states")
            }
        }
    }
}

impl std::error::Error for GraphBuildError {}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Lowered instant instructions and bindings; compile-time proofs are not retained.
struct InstantProgram {
    /// These reduced pull waves replace covered computation while retaining ordinary reset work.
    kernels: Box<[std::sync::Arc<InstantKernel>]>,
    /// This maps each eligible first request to its immutable wave program.
    kernel_by_actor: Box<[Option<u32>]>,
    /// These private-cell programs reuse the existing actor, resource, and clock state.
    proxies: Box<[InstantProxy]>,
    /// This certifies that no foreign base, head, or resource can reach a private payload's contacts.
    private_resource_owners: Box<[Option<PistonId>]>,
    /// This binds a route directly to its proxy and ordered instruction sequence.
    route_by_route: Box<[Option<(u32, u32)>]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Immutable tables used by the graph executor after lowering.
pub(crate) struct CircuitProgram {
    /// Fixed electrical reads inside otherwise general mixed-circuit execution.
    mixed_classic: MixedClassicProgram,
    fixed_classic: Option<std::sync::Arc<FixedClassicProgram>>,
    fixed_signals: FixedSignalProgram,
    classic: ClassicProgram,
    instant: InstantProgram,
    nodes: Box<[NodeSpec]>,
    power_links: Box<[PowerLink]>,
    power_inputs: FxHashMap<PowerTarget, Range<usize>>,
    comparator_inputs: Box<[ComparatorInputPlan]>,
    /// This maps a dense node ID directly to its comparator input plan.
    comparator_plan_by_node: Box<[Option<u32>]>,
    profiles: Box<[Profile]>,
    profile_blocks: Box<[Block]>,
    guards: Box<[Guard]>,
    contacts: Box<[ContactSpec]>,
    routes: Box<[Route]>,
    /// This maps each wire plan to ordered slices in its Turbo delivery route.
    turbo_callback_indices: Box<[Option<TurboCallbackIndex>]>,
    wires: Box<[WirePlan]>,
    /// These immutable certificates select the local power calculation for proven wire paths.
    dust_paths: Box<[DustPathPlan]>,
    /// This binds each original wire plan to its optional dust-path certificate.
    dust_path_by_wire: Box<[Option<DustPathId>]>,
    sites: Box<[SiteSpec]>,
    pistons: Box<[PistonSpec]>,
    resources: Box<[ResourceSpec]>,
    /// This binds each piston to the lane resource selected during lowering.
    piston_resources: Box<[Option<ResourceId>]>,
    /// These candidates contain resources that can occupy each contact.
    resource_contacts: ContactCandidates<ResourceId>,
    /// These candidates contain piston bases indexed by contact.
    base_pistons: ContactCandidates<PistonId>,
    /// These candidates contain piston heads indexed by their near contact.
    head_pistons: ContactCandidates<PistonId>,
    command_blocks: Box<[CommandBlockPlan]>,
    /// This maps a dense node ID directly to its command plan.
    command_plan_by_node: Box<[Option<u32>]>,
    note_blocks: Box<[NoteBlockPlan]>,
    /// This maps a dense node ID directly to its note-block plan.
    note_plan_by_node: Box<[Option<u32>]>,
}

impl CircuitProgram {
    pub(crate) fn mixed_classic(&self) -> &MixedClassicProgram {
        &self.mixed_classic
    }
    pub(crate) fn instant_kernels(&self) -> &[std::sync::Arc<InstantKernel>] {
        &self.instant.kernels
    }

    pub(crate) fn instant_kernel_for_actor(&self, actor: PistonId) -> Option<&std::sync::Arc<InstantKernel>> {
        self.instant.kernel_by_actor[actor.index()].map(|index| &self.instant.kernels[index as usize])
    }
    #[cfg(test)]
    pub(crate) fn without_fixed_wire_callbacks(&self) -> Self {
        let mut general = self.clone();
        // Rebuild the same pre-specialization index so controls do not pay a
        // deliberately slower full-route scan after the optimized table is gone.
        general.turbo_callback_indices = general
            .wires
            .iter()
            .map(|wire| crate::redpiler::passes::index_turbo_callbacks::index_wire(wire, &general.routes))
            .collect();
        for wire in &mut general.wires {
            wire.execution.callback_selection = None;
        }
        general
    }
    pub(crate) fn fixed_signal_query(&self, target: PowerTarget) -> Option<(u8, &[WeightedSignal])> {
        self.fixed_signals.query(target)
    }
    pub(crate) fn fixed_classic(&self) -> Option<&std::sync::Arc<FixedClassicProgram>> {
        self.fixed_classic.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn without_fixed_signal_queries(mut self) -> Self {
        self.fixed_signals = FixedSignalProgram::default();
        self
    }
    pub(crate) fn classic(&self) -> &ClassicProgram { &self.classic }
    pub(crate) fn private_resource_owner(&self, resource: ResourceId) -> Option<PistonId> {
        self.instant.private_resource_owners[resource.index()]
    }
    pub(crate) fn instant_proxies(&self) -> &[InstantProxy] {
        &self.instant.proxies
    }

    pub(crate) fn instant_route_binding(&self, route: RouteId) -> Option<(u32, u32)> {
        self.instant.route_by_route[route.index()]
    }
    pub(crate) fn nodes(&self) -> &[NodeSpec] {
        &self.nodes
    }

    pub(crate) fn power_links(&self) -> &[PowerLink] {
        &self.power_links
    }

    pub(crate) fn power_links_to(&self, target: PowerTarget) -> &[PowerLink] {
        let Some(range) = self.power_inputs.get(&target) else {
            return &[];
        };
        &self.power_links[range.clone()]
    }

    pub(crate) fn profile_block(&self, profile: ProfileId) -> Option<Block> {
        self.profile_blocks.get(profile.index()).copied()
    }

    pub(crate) fn comparator_inputs(&self) -> &[ComparatorInputPlan] {
        &self.comparator_inputs
    }

    pub(crate) fn comparator_input(&self, node: NodeId) -> Option<&ComparatorInputPlan> {
        let index = self.comparator_plan_by_node.get(node.index()).copied().flatten()?;
        self.comparator_inputs.get(index as usize)
    }

    pub(crate) fn sites(&self) -> &[SiteSpec] {
        &self.sites
    }

    pub(crate) fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub(crate) fn guards(&self) -> &[Guard] {
        &self.guards
    }

    pub(crate) fn contacts(&self) -> &[ContactSpec] {
        &self.contacts
    }

    pub(crate) fn routes(&self) -> &[Route] {
        &self.routes
    }

    pub(crate) fn wires(&self) -> &[WirePlan] {
        &self.wires
    }

    pub(crate) fn turbo_callback_index(&self, wire_index: usize) -> Option<&TurboCallbackIndex> {
        self.turbo_callback_indices.get(wire_index)?.as_ref()
    }

    pub(crate) fn turbo_callback_indices(&self) -> &[Option<TurboCallbackIndex>] {
        &self.turbo_callback_indices
    }

    pub(crate) fn dust_paths(&self) -> &[DustPathPlan] {
        &self.dust_paths
    }

    pub(crate) fn dust_path_for_wire(&self, node: NodeId) -> Option<&DustPathPlan> {
        let CircuitNodeKind::Wire(wire_index) = self.nodes.get(node.index())?.kind else {
            return None;
        };
        let id = self.dust_path_by_wire.get(wire_index).copied().flatten()?;
        self.dust_paths.get(id.index())
    }

    pub(crate) fn pistons(&self) -> &[PistonSpec] {
        &self.pistons
    }

    pub(crate) fn resources(&self) -> &[ResourceSpec] {
        &self.resources
    }

    pub(crate) fn resource_for_piston(&self, piston: PistonId) -> Option<ResourceId> {
        self.piston_resources.get(piston.index()).copied().flatten()
    }

    pub(crate) fn resources_at_contact(&self, contact: ContactId) -> &[ResourceId] {
        self.resource_contacts.at(contact)
    }

    pub(crate) fn base_pistons_at_contact(&self, contact: ContactId) -> &[PistonId] {
        self.base_pistons.at(contact)
    }

    pub(crate) fn head_pistons_at_contact(&self, contact: ContactId) -> &[PistonId] {
        self.head_pistons.at(contact)
    }

    pub(crate) fn command_blocks(&self) -> &[CommandBlockPlan] {
        &self.command_blocks
    }

    pub(crate) fn command_plan(&self, node: NodeId) -> Option<CommandBlockPlan> {
        let index = self.command_plan_by_node.get(node.index()).copied().flatten()?;
        self.command_blocks.get(index as usize).copied()
    }

    pub(crate) fn note_blocks(&self) -> &[NoteBlockPlan] {
        &self.note_blocks
    }

    pub(crate) fn note_plan(&self, node: NodeId) -> Option<NoteBlockPlan> {
        let index = self.note_plan_by_node.get(node.index()).copied().flatten()?;
        self.note_blocks.get(index as usize).copied()
    }

    pub(crate) fn position_index(&self) -> FxHashMap<BlockPos, NodeId> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                if !matches!(
                    node.kind,
                    CircuitNodeKind::Electrical(_)
                        | CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::BuiltinReset { .. }
                        | CircuitNodeKind::Wire(_)
                ) {
                    return None;
                }
                Some((node.position?, NodeId::new(index as u32)))
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Mutable component, piston, and resource state paired with a circuit program.
pub(crate) struct CircuitState {
    pub(crate) classic_signals: Box<[u8]>,
    pub(crate) classic_active: Box<[bool]>,
    pub(crate) components: Box<[ComponentState]>,
    pub(crate) pistons: Box<[PistonState]>,
    pub(crate) resources: Box<[ResourceState]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Properties captured from the block occupying a compiled site.
pub(crate) enum ObservedProperties {
    Passive,
    Electrical { powered: bool, locked: bool },
    CopperBulb { powered: bool, lit: bool },
    Wire(RedstoneWire),
    Piston { extended: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Captured profile and callback receiver for one observed contact.
pub(crate) struct ObservedSite {
    pub(crate) profile: Option<ProfileId>,
    pub(crate) receiver: Option<CallbackReceiver>,
    pub(crate) properties: ObservedProperties,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One sampled neighbor used by the runtime wire walk.
pub(crate) struct WalkSite {
    pub(crate) site: SiteId,
    pub(crate) observed: ObservedSite,
    pub(crate) visited: bool,
    pub(crate) x_bias: i32,
    pub(crate) z_bias: i32,
    pub(crate) layer: u32,
    pub(crate) heading: Option<u8>,
    /// This indexes a compiled walk's cached neighborhood; general walks leave it empty.
    pub(crate) neighbor_slots: Option<u32>,
}

/// Per-invocation dust scratch. It is constructed separately from persistent
/// circuit state, so nested walks cannot overwrite a caller's captured sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireWalk {
    pub(crate) site_slots: FxHashMap<SiteId, usize>,
    /// This reusable dense index replaces hashing in compiled walks; MAX means undiscovered.
    direct_slots: Vec<u32>,
    /// These arrays retain discovered neighbor slots without repeating 24 address lookups.
    neighbor_slots: Vec<[usize; 24]>,
    // shortcut: inline storage covers the root plus one 24-neighbor set; larger walks spill.
    pub(crate) sites: SmallVec<[WalkSite; 25]>,
    pub(crate) queues: [VecDeque<usize>; 3],
    pub(crate) layer: u32,
}

impl WireWalk {
    pub(crate) fn new() -> Self {
        Self {
            site_slots: FxHashMap::default(),
            direct_slots: Vec::new(),
            neighbor_slots: Vec::new(),
            sites: SmallVec::new(),
            queues: std::array::from_fn(|_| VecDeque::new()),
            layer: 0,
        }
    }

    pub(crate) fn compiled(site_count: usize) -> Self {
        Self {
            direct_slots: vec![u32::MAX; site_count],
            ..Self::new()
        }
    }

    pub(crate) fn is_compiled(&self) -> bool {
        !self.direct_slots.is_empty()
    }

    pub(crate) fn slot(&self, site: SiteId) -> Option<usize> {
        if self.is_compiled() {
            let slot = self.direct_slots[site.index()];
            (slot != u32::MAX).then_some(slot as usize)
        } else {
            self.site_slots.get(&site).copied()
        }
    }

    pub(crate) fn cached_neighbors(&self, root: usize) -> Option<[usize; 24]> {
        self.sites[root].neighbor_slots.map(|index| self.neighbor_slots[index as usize])
    }

    pub(crate) fn cache_neighbors(&mut self, root: usize, slots: [usize; 24]) {
        if self.is_compiled() {
            self.sites[root].neighbor_slots = Some(self.neighbor_slots.len() as u32);
            self.neighbor_slots.push(slots);
        }
    }

    /// Discard this invocation's observations, retaining only reusable allocation capacity.
    pub(crate) fn clear(&mut self) {
        if self.is_compiled() {
            for site in &self.sites {
                self.direct_slots[site.site.index()] = u32::MAX;
            }
        }
        self.site_slots.clear();
        self.sites.clear();
        self.neighbor_slots.clear();
        for queue in &mut self.queues {
            queue.clear();
        }
        self.layer = 0;
    }

    pub(crate) fn insert_site(&mut self, site: SiteId, walk_site: WalkSite) -> usize {
        debug_assert!(self.slot(site).is_none());
        debug_assert_eq!(walk_site.site, site);
        let slot = self.sites.len();
        if self.is_compiled() {
            self.direct_slots[site.index()] = u32::try_from(slot).expect("validated site count fits u32");
        } else {
            self.site_slots.insert(site, slot);
        }
        self.sites.push(walk_site);
        slot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_with_mixed_links() -> CircuitDraft {
        let guard = GuardId::new(0);
        let site = SiteId::new(0);
        let contact = ContactId::new(0);
        let second_contact = ContactId::new(1);
        let piston = PistonId::new(0);
        let route = RouteId::try_from_index(0).unwrap();
        let profile = ProfileId::new(0);
        let piston_node = NodeId::new(2);
        let contact_node = NodeId::new(3);
        let draft = CircuitDraft {
            nodes: vec![
                CircuitNodeDraft {
                    position: Some(BlockPos::new(0, 0, 0)),
                    raw_state: Some(mchprs_blocks::blocks::Block::Lever { lever: Default::default() }.get_id()),
                    kind: CircuitNodeKind::Electrical(NodeType::Lever),
                    state: ComponentState::Electrical(NodeState::default()),
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(1, 0, 0)),
                    raw_state: Some(mchprs_blocks::blocks::Block::RedstoneLamp { lit: false }.get_id()),
                    kind: CircuitNodeKind::Electrical(NodeType::Lamp),
                    state: ComponentState::Electrical(NodeState::default()),
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(2, 0, 0)),
                    raw_state: None,
                    kind: CircuitNodeKind::Piston(piston),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(3, 0, 0)),
                    raw_state: None,
                    kind: CircuitNodeKind::Contact(contact),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(4, 0, 0)),
                    raw_state: None,
                    kind: CircuitNodeKind::Contact(second_contact),
                    state: ComponentState::Specialized,
                    output_route: None,
                },
                CircuitNodeDraft {
                    position: Some(BlockPos::new(0, 0, 1)),
                    raw_state: Some(mchprs_blocks::blocks::Block::RedstoneWire { wire: Default::default() }.get_id()),
                    kind: CircuitNodeKind::Wire(0),
                    state: ComponentState::Wire(RedstoneWire {
                        power: 4,
                        ..Default::default()
                    }),
                    output_route: None,
                },
            ],
            links: vec![
                LinkSpec {
                    from: NodeId::new(2),
                    to: NodeId::new(1),
                    kind: LinkKind::Update {
                        direction: Some(BlockFace::East),
                        guard,
                    },
                    callback: CallbackEdgeRef { route, sequence: 0 },
                },
                LinkSpec {
                    from: NodeId::new(2),
                    to: NodeId::new(1),
                    kind: LinkKind::Update {
                        direction: Some(BlockFace::East),
                        guard,
                    },
                    callback: CallbackEdgeRef { route, sequence: 1 },
                },
                LinkSpec {
                    from: NodeId::new(2),
                    to: NodeId::new(1),
                    kind: LinkKind::Shape {
                        direction: BlockFace::West,
                        guard,
                    },
                    callback: CallbackEdgeRef { route, sequence: 2 },
                },
            ],
            power_links: vec![PowerLink {
                from: site,
                from_port: PowerPort::Output,
                to: PowerTarget::Input {
                    node: NodeId::new(0),
                    port: InputPort::Main,
                },
                face: BlockFace::East,
                attenuation: 15,
                guard,
            }],
            comparator_inputs: vec![],
            profiles: vec![Profile {
                raw_state: 0,
                registry_type: 0,
                solid: false,
                transparent: true,
                cube: false,
                electrical: ProfileElectrical::None,
            }],
            guards: vec![Guard::Always],
            contacts: vec![
                ContactSpec {
                    node: contact_node,
                    position: BlockPos::new(3, 0, 0),
                    variants: vec![ContactVariant {
                        when: guard,
                        profile,
                        receiver: None,
                    }]
                    .into_boxed_slice(),
                },
                ContactSpec {
                    node: NodeId::new(4),
                    position: BlockPos::new(4, 0, 0),
                    variants: vec![ContactVariant {
                        when: guard,
                        profile,
                        receiver: None,
                    }]
                    .into_boxed_slice(),
                },
            ],
            routes: vec![Route {
                source: piston_node,
                deliveries: vec![
                    RouteDelivery {
                        target: site,
                        kind: DeliveryKind::Update {
                            direction: Some(BlockFace::East),
                        },
                        guard,
                        view: OperandView::Live,
                        sequence: 0,
                        receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
                    },
                    RouteDelivery {
                        target: site,
                        kind: DeliveryKind::Update {
                            direction: Some(BlockFace::East),
                        },
                        guard,
                        view: OperandView::Live,
                        sequence: 1,
                        receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
                    },
                    RouteDelivery {
                        target: site,
                        kind: DeliveryKind::Shape { direction: BlockFace::West },
                        guard,
                        view: OperandView::Live,
                        sequence: 2,
                        receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
                    },
                ]
                .into_boxed_slice(),
            }],
            wires: vec![WirePlan {
                node: NodeId::new(5),
                canonical_neighbors: [site; 24],
                shape_route: None,
                turbo_route: None,
                execution: Default::default(),
            }],

            wire_relations: vec![],
            sites: vec![SiteSpec {
                position: BlockPos::new(0, 0, 0),
                source: SiteSource::Component(NodeId::new(1)),
                source_strength: 0,
            }],
            pistons: vec![PistonSpec {
                node: piston_node,
                position: BlockPos::new(2, 0, 0),
                facing: BlockFace::East,
                sticky: false,
                base: contact,
                near: contact,
                far: contact,
                lane: PistonLane::Empty,
                routes: PistonRoutes {
                    base_changed: route,
                    near_changed: route,
                    far_changed: None,
                    far_shape_changed: None,
                    base_restored: route,
                    head_restored: route,
                    resource_restored_near: None,
                    resource_restored_far: None,
                },
            }],
            resources: vec![ResourceSpec {
                material: profile,
                contacts: vec![contact, second_contact].into_boxed_slice(),
            }],
            initial_pistons: vec![PistonState {
                extended: false,
                head_present: false,
                availability: PistonAvailability::Ready,
                revision: 0,
            }],

            plans: crate::redpiler::program_graph::ExecutionPlans {
                turbo_callback_indices: vec![None],
                ..Default::default()
            },
            initial_resources: vec![ResourceState {
                at: contact,
                enabled: true,
                revision: 0,
            }],
        };
        draft
    }

    #[test]
    fn lowering_preserves_callback_duplicates_and_direction() {
        let draft = draft_with_mixed_links();
        draft.validate().unwrap();
        assert_eq!(draft.links.len(), 3);
        assert_eq!(draft.power_links.len(), 1);
        assert_eq!(draft.links.iter().filter(|link| matches!(link.kind, LinkKind::Update { .. })).count(), 2);
        assert!(matches!(
            draft.links[2].kind,
            LinkKind::Shape {
                direction: BlockFace::West,
                ..
            }
        ));
        let (program, state) = draft.lower().unwrap();
        assert_eq!(
            program.routes[0].deliveries[0].kind,
            DeliveryKind::Update {
                direction: Some(BlockFace::East)
            }
        );
        assert_eq!(
            program.routes[0].deliveries[1].kind,
            DeliveryKind::Update {
                direction: Some(BlockFace::East)
            }
        );
        assert_eq!(program.routes[0].deliveries[2].kind, DeliveryKind::Shape { direction: BlockFace::West });
        assert_eq!(state.pistons.len(), 1);
        assert_eq!(state.resources.len(), 1);
        assert_eq!(state.resources[0].at, ContactId::new(0));
        assert!(matches!(&state.components[5], ComponentState::Wire(wire) if wire.power == 4));
    }

    #[test]
    fn invalid_link_reference_and_strength_fail_before_program_creation() {
        let mut invalid_reference = draft_with_mixed_links();
        invalid_reference.links[0].to = NodeId::new(99);
        assert!(matches!(
            invalid_reference.lower(),
            Err(GraphBuildError::InvalidReference { table: "nodes", .. })
        ));

        let mut invalid_strength = draft_with_mixed_links();
        invalid_strength.nodes[0].state = ComponentState::Electrical(NodeState::with_strength(16));
        assert!(matches!(
            invalid_strength.lower(),
            Err(GraphBuildError::InvalidStrength { strength: 16, .. })
        ));

        let mut invalid_piston_position = draft_with_mixed_links();
        invalid_piston_position.pistons[0].position = BlockPos::new(3, 0, 0);
        assert!(matches!(
            invalid_piston_position.lower(),
            Err(GraphBuildError::NodeKindMismatch { node }) if node == NodeId::new(2)
        ));

        let mut invalid_attenuation = draft_with_mixed_links();
        invalid_attenuation.power_links[0].attenuation = 16;
        assert!(matches!(
            invalid_attenuation.lower(),
            Err(GraphBuildError::InvalidPowerAttenuation { attenuation: 16, .. })
        ));

        let mut invalid_callback = draft_with_mixed_links();
        invalid_callback.links[1].callback = CallbackEdgeRef {
            route: RouteId::try_from_index(0).unwrap(),
            sequence: 99,
        };
        assert!(matches!(invalid_callback.lower(), Err(GraphBuildError::CallbackEdgeMismatch { .. })));
    }

    #[test]
    fn duplicate_callback_delivery_without_a_matching_edge_is_rejected() {
        let mut duplicate = draft_with_mixed_links();
        let duplicate_delivery = duplicate.routes[0].deliveries[0];
        let mut deliveries = duplicate.routes[0].deliveries.to_vec();
        deliveries.insert(1, duplicate_delivery);
        duplicate.routes[0].deliveries = deliveries.into_boxed_slice();

        assert!(matches!(
            duplicate.lower(),
            Err(GraphBuildError::CallbackEdgeMismatch { sequence: 0, .. })
        ));
    }

    #[test]
    fn callback_link_permutations_preserve_ordered_delivery_multiplicity() {
        let draft = draft_with_mixed_links();
        let expected = draft.clone().lower().unwrap();
        for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
            let mut permuted = draft.clone();
            permuted.links = order.iter().map(|&index| draft.links[index]).collect();
            assert_eq!(permuted.lower().unwrap(), expected);
        }
    }

    #[test]
    fn callback_link_stream_and_multiset_reject_the_same_corruptions() {
        let draft = draft_with_mixed_links();
        for reverse in [false, true] {
            for case in 0..6 {
                let mut invalid = draft.clone();
                match case {
                    0 => { invalid.links.pop(); }
                    1 => invalid.links.push(invalid.links[0]),
                    2 => invalid.links[1] = invalid.links[0],
                    3 => invalid.links[0].from = NodeId::new(99),
                    4 => invalid.links[0].kind = LinkKind::Update { direction: Some(BlockFace::North), guard: GuardId::new(0) },
                    5 => invalid.links[0].callback.sequence = 99,
                    _ => unreachable!(),
                }
                if reverse { invalid.links.reverse(); }
                assert!(invalid.lower().is_err(), "corruption {case}, reverse={reverse}");
            }
        }
    }

    #[test]
    fn callback_link_validation_counts_identical_keys_and_skips_receiverless_rows() {
        let mut duplicate = draft_with_mixed_links();
        duplicate.routes[0].deliveries[1].sequence = 0;
        duplicate.links[1].callback.sequence = 0;
        duplicate.validate().unwrap();
        duplicate.links.reverse();
        duplicate.validate().unwrap();
        duplicate.links.pop();
        assert!(duplicate.validate().is_err());

        let mut receiverless = draft_with_mixed_links();
        receiverless.routes[0].deliveries[0].receiver = None;
        receiverless.links.remove(0);
        receiverless.validate().unwrap();
        receiverless.links.reverse();
        receiverless.validate().unwrap();
    }

    #[test]
    fn resource_lane_requires_both_destination_restoration_routes() {
        let mut missing = draft_with_mixed_links();
        missing.pistons[0].lane = PistonLane::SingleResource { far: ContactId::new(0) };
        assert!(matches!(missing.lower(), Err(GraphBuildError::PistonResourceRestorationMismatch { .. })));
    }

    #[test]
    fn resource_lane_far_contact_must_match_piston_far_contact() {
        let mut mismatch = draft_with_mixed_links();
        mismatch.pistons[0].lane = PistonLane::SingleResource { far: ContactId::new(1) };
        mismatch.pistons[0].routes.resource_restored_near = Some(RouteId::try_from_index(0).unwrap());
        mismatch.pistons[0].routes.resource_restored_far = Some(RouteId::try_from_index(0).unwrap());

        assert!(matches!(
            mismatch.lower(),
            Err(GraphBuildError::PistonFarContactMismatch { node }) if node == NodeId::new(2)
        ));
    }

    #[test]
    fn invalid_contact_binding_and_guard_cycle_are_rejected() {
        let mut invalid_contact = draft_with_mixed_links();
        invalid_contact.contacts[0].node = NodeId::new(1);
        assert!(matches!(invalid_contact.lower(), Err(GraphBuildError::NodeKindMismatch { .. })));

        let mut invalid_state = draft_with_mixed_links();
        invalid_state.nodes[2].state = ComponentState::Electrical(NodeState::default());
        match invalid_state.lower() {
            Err(GraphBuildError::InvalidNodeState { node }) => {
                assert_eq!(node, NodeId::new(2));
            }
            result => panic!("unexpected validation result: {result:?}"),
        }

        let mut cyclic_guards = draft_with_mixed_links();
        cyclic_guards.guards = vec![Guard::Not(GuardId::new(0))];
        assert!(matches!(cyclic_guards.lower(), Err(GraphBuildError::CyclicGuard { .. })));

        let mut self_dependent_contact = draft_with_mixed_links();
        let site = SiteId::new(1);
        self_dependent_contact.sites.push(SiteSpec {
            position: BlockPos::new(3, 1, 0),
            source: SiteSource::Contact(ContactId::new(0)),
            source_strength: 0,
        });
        self_dependent_contact.guards.push(Guard::Site {
            site,
            view: OperandView::Live,
            test: SiteTest::Solid,
        });
        self_dependent_contact.contacts[0].variants[0].when = GuardId::new(1);
        assert!(matches!(
            self_dependent_contact.lower(),
            Err(GraphBuildError::ContactGuardReadsSite {
                contact: ContactId(0),
                site: SiteId(1),
                ..
            })
        ));

        let mut conflicting_resources = draft_with_mixed_links();
        conflicting_resources.resources.push(ResourceSpec {
            material: ProfileId::new(0),
            contacts: vec![ContactId::new(0), ContactId::new(1)].into_boxed_slice(),
        });
        conflicting_resources.initial_resources.push(ResourceState {
            at: ContactId::new(1),
            enabled: true,
            revision: 0,
        });
        assert!(matches!(
            conflicting_resources.lower(),
            Err(GraphBuildError::ResourcesShareContact { .. })
        ));
    }

    #[test]
    fn walk_state_is_fresh_and_separate_from_circuit_state() {
        let mut outer = WireWalk::new();
        let mut nested = WireWalk::new();
        let site = SiteId::new(0);
        outer.insert_site(
            site,
            WalkSite {
                site,
                observed: ObservedSite {
                    profile: None,
                    receiver: None,
                    properties: ObservedProperties::Passive,
                },
                visited: true,
                x_bias: 1,
                z_bias: 0,
                layer: 2,
                heading: Some(1),
                neighbor_slots: None,
            },
        );
        assert!(nested.sites.is_empty());
        nested.insert_site(
            site,
            WalkSite {
                site,
                observed: ObservedSite {
                    profile: None,
                    receiver: None,
                    properties: ObservedProperties::Passive,
                },
                visited: false,
                x_bias: 0,
                z_bias: 0,
                layer: 0,
                heading: None,
                neighbor_slots: None,
            },
        );
        assert_eq!(outer.site_slots[&site], 0);
        assert_eq!(nested.site_slots[&site], 0);
        assert!(outer.sites[0].visited);
        assert!(!nested.sites[0].visited);
        assert_eq!(outer.queues, std::array::from_fn(|_| VecDeque::new()));
        assert_eq!(nested.queues, std::array::from_fn(|_| VecDeque::new()));
    }

    #[test]
    fn walk_slots_keep_the_initial_neighbor_set_inline() {
        let mut walk = WireWalk::new();
        for index in 0..25 {
            let site = SiteId::new(index);
            walk.insert_site(
                site,
                WalkSite {
                    site,
                    observed: ObservedSite {
                        profile: None,
                        receiver: None,
                        properties: ObservedProperties::Passive,
                    },
                    visited: false,
                    x_bias: 0,
                    z_bias: 0,
                    layer: 0,
                    heading: None,
                    neighbor_slots: None,
                },
            );
        }
        assert_eq!(walk.site_slots.len(), 25);
        assert!(!walk.sites.spilled());
        let site = SiteId::new(25);
        walk.insert_site(
            site,
            WalkSite {
                site,
                observed: ObservedSite {
                    profile: None,
                    receiver: None,
                    properties: ObservedProperties::Passive,
                },
                visited: false,
                x_bias: 0,
                z_bias: 0,
                layer: 0,
                heading: None,
                neighbor_slots: None,
            },
        );
        assert!(walk.sites.spilled());
    }

    #[test]
    fn deep_audit_missing_duplicate_callback_edge_returns_error_without_panicking() {
        let mut draft = draft_with_mixed_links();
        draft.routes[0].deliveries[1].sequence = 0;
        draft.links.retain(|link| {
            link.callback
                != CallbackEdgeRef {
                    route: RouteId::try_from_index(0).unwrap(),
                    sequence: 1,
                }
        });
        let result = std::panic::catch_unwind(|| draft.lower());
        assert!(result.is_ok(), "invalid multiplicity must return a validation error, not panic");
        assert!(matches!(result.unwrap(), Err(GraphBuildError::CallbackEdgeMismatch { .. })));
    }

    fn deep_audit_contact_delivery_draft(bypass_selection: bool) -> CircuitDraft {
        let mut draft = draft_with_mixed_links();
        let selector = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(Guard::State(StateTest::ResourceEnabled(ResourceId::new(0))));
        draft.contacts[0].variants[0].when = selector;
        draft.contacts[0].variants[0].receiver = Some(CallbackReceiver::Component(NodeId::new(1)));
        draft.sites[0].source = SiteSource::Contact(ContactId::new(0));
        let delivery_guard = GuardId::new(draft.guards.len() as u32);
        draft.guards.push(if bypass_selection {
            Guard::Any(vec![selector, GuardId::new(0)].into_boxed_slice())
        } else {
            Guard::Not(selector)
        });
        let mut delivery = draft.routes[0].deliveries[0];
        delivery.guard = delivery_guard;
        draft.routes[0].deliveries = vec![delivery].into_boxed_slice();
        draft.links.retain(|link| {
            link.callback
                == CallbackEdgeRef {
                    route: RouteId::try_from_index(0).unwrap(),
                    sequence: 0,
                }
        });
        for link in &mut draft.links {
            if let LinkKind::Update { guard, .. } = &mut link.kind {
                *guard = delivery_guard;
            }
        }
        draft
    }

    #[test]
    fn deep_audit_negated_receiver_selection_is_rejected() {
        assert!(
            matches!(
                deep_audit_contact_delivery_draft(false).lower(),
                Err(GraphBuildError::CallbackEdgeMismatch { .. })
            ),
            "containing a selector under Not does not imply that receiver is selected"
        );
    }

    #[test]
    fn deep_audit_optional_receiver_selection_is_rejected() {
        assert!(
            matches!(
                deep_audit_contact_delivery_draft(true).lower(),
                Err(GraphBuildError::CallbackEdgeMismatch { .. })
            ),
            "Any(selector, Always) permits delivering to an unselected receiver"
        );
    }

    #[test]
    fn deep_audit_component_sites_cannot_alias_specialized_owners() {
        for target in [NodeId::new(2), NodeId::new(3)] {
            let mut draft = draft_with_mixed_links();
            draft.sites[0].source = SiteSource::Component(target);
            for delivery in draft.routes[0].deliveries.iter_mut() {
                delivery.receiver = Some(CallbackReceiver::Component(target));
            }
            for link in draft.links.iter_mut() {
                link.to = target;
            }
            assert!(
                matches!(
                    draft.lower(),
                    Err(GraphBuildError::InvalidComponentBinding { node }) if node == target
                ),
                "piston/contact owners need their specialized site and receiver bindings: {target:?}"
            );

            let mut receiver_alias = draft_with_mixed_links();
            for delivery in receiver_alias.routes[0].deliveries.iter_mut() {
                delivery.receiver = Some(CallbackReceiver::Component(target));
            }
            for link in receiver_alias.links.iter_mut() {
                link.to = target;
            }
            assert!(
                matches!(
                    receiver_alias.lower(),
                    Err(GraphBuildError::InvalidComponentBinding { node }) if node == target
                ),
                "component receivers cannot alias specialized owners: {target:?}"
            );
        }
    }
}
