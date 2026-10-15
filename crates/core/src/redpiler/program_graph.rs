//! Typed construction and immutable program storage for the general graph.

mod instant_machine;
mod instant_regions;
pub(crate) use instant_machine::*;
mod wire_query_facts;
pub(crate) use instant_regions::*;
pub(crate) use wire_query_facts::WireQueryFacts;

use crate::redpiler::components::{NodeState, NodeType};
use crate::redpiler::passes::index_turbo_callbacks::TurboCallbackIndex;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// All typed tables assembled during graph preparation.
pub(crate) struct CircuitDraft {
    pub(crate) nodes: Vec<CircuitNodeDraft>,
    pub(crate) links: Vec<LinkSpec>,
    pub(crate) power_links: Vec<PowerLink>,
    pub(crate) comparator_inputs: Vec<ComparatorInputPlan>,
    pub(crate) profiles: Vec<Profile>,
    pub(crate) guards: Vec<Guard>,
    pub(crate) contacts: Vec<ContactSpec>,
    pub(crate) routes: Vec<Route>,
    /// An empty table disables indexing; otherwise this is aligned with wire plans.
    pub(crate) turbo_callback_indices: Vec<Option<TurboCallbackIndex>>,
    pub(crate) wires: Vec<WirePlan>,
    pub(crate) dust_paths: Vec<DustPathPlan>,
    pub(crate) wire_relations: Vec<WireRelation>,
    pub(crate) sites: Vec<SiteSpec>,
    pub(crate) pistons: Vec<PistonSpec>,
    pub(crate) resources: Vec<ResourceSpec>,
    pub(crate) initial_pistons: Vec<PistonState>,
    pub(crate) initial_resources: Vec<ResourceState>,
    /// These compile-time boundaries expose every owned state and crossing notification.
    pub(crate) instant_regions: Vec<InstantRegion>,
    /// These certificates replace private-cell internal dispatch with direct instructions.
    pub(crate) instant_proxies: Vec<InstantProxy>,
    /// This optional bounded machine replaces closed-selection transitions without duplicating live state.
    pub(crate) instant_machine: Option<std::sync::Arc<InstantMachine>>,
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
    pub(crate) fn validate(&self) -> Result<(), GraphBuildError> {
        self.validate_inner()
    }

    pub(crate) fn lower(mut self) -> Result<(CircuitProgram, CircuitState), GraphBuildError> {
        self.validate_inner()?;

        let mut instant_route_by_route = vec![None; self.routes.len()];
        let mut private_resource_owners = vec![None; self.resources.len()];
        for (proxy_index, proxy) in self.instant_proxies.iter().enumerate() {
            for cell in &proxy.private_cells {
                private_resource_owners[cell.resource.index()] = Some(cell.actor);
            }
            for (route_index, route) in proxy.routes.iter().enumerate() {
                instant_route_by_route[route.route.index()] = Some((proxy_index as u32, route_index as u32));
                // The validated instructions retain every witness; runtime needs only one copy.
                self.routes[route.route.index()].deliveries = Box::new([]);
            }
        }

        let nodes = self
            .nodes
            .iter()
            .map(|node| NodeSpec {
                position: node.position,
                raw_state: node.raw_state,
                block: node.raw_state.map(Block::from_id),
                kind: node.kind.clone(),
                output_route: node.output_route,
            })
            .collect::<Vec<_>>();

        self.power_links.sort_by_key(|link| link.to);
        let mut power_inputs: FxHashMap<PowerTarget, Range<usize>> = FxHashMap::default();
        for (index, link) in self.power_links.iter().enumerate() {
            power_inputs.entry(link.to).or_insert(index..index).end = index + 1;
        }
        let profile_blocks = self
            .profiles
            .iter()
            .map(|profile| Block::from_id(profile.raw_state))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let node_positions: FxHashMap<_, _> = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some((node.position?, NodeId::try_from_index(index)?)))
            .collect();
        let site_positions: FxHashMap<_, _> = self
            .sites
            .iter()
            .enumerate()
            .filter_map(|(index, site)| Some((site.position, SiteId::try_from_index(index)?)))
            .collect();
        let command_blocks = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) = node.kind else {
                    return None;
                };
                let id = NodeId::try_from_index(index)?;
                let block = node.block?;
                let position = node.position?;
                let facing = command_facing(block);
                let command_at = |position: BlockPos| {
                    node_positions
                        .get(&position)
                        .copied()
                        .filter(|&other| matches!(nodes[other.index()].kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })))
                };
                let previous = command_at(position.offset(facing.opposite()));
                let next = command_at(position.offset(facing)).filter(|&other| {
                    matches!(
                        nodes[other.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::CommandBlock { chain: true, .. })
                    )
                });
                Some(CommandBlockPlan {
                    node: id,
                    previous,
                    next,
                    conditional: block.property("conditional") == Some("true"),
                })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let note_blocks = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                if !matches!(node.kind, CircuitNodeKind::Electrical(NodeType::NoteBlock { .. })) {
                    return None;
                }
                let id = NodeId::try_from_index(index)?;
                let position = node.position?;
                Some(NoteBlockPlan {
                    node: id,
                    below: *site_positions.get(&position.offset(BlockFace::Bottom))?,
                    above: *site_positions.get(&position.offset(BlockFace::Top))?,
                })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let mut command_plan_by_node = vec![None; nodes.len()];
        for (index, plan) in command_blocks.iter().enumerate() {
            command_plan_by_node[plan.node.index()] = Some(index as u32);
        }
        let mut note_plan_by_node = vec![None; nodes.len()];
        for (index, plan) in note_blocks.iter().enumerate() {
            note_plan_by_node[plan.node.index()] = Some(index as u32);
        }
        let mut comparator_plan_by_node = vec![None; nodes.len()];
        for (index, plan) in self.comparator_inputs.iter().enumerate() {
            comparator_plan_by_node[plan.node.index()].get_or_insert(index as u32);
        }
        let mut piston_resources = Vec::with_capacity(self.pistons.len());
        for piston in &self.pistons {
            let binding = match piston.lane {
                PistonLane::Empty => None,
                PistonLane::SingleResource { far } => self
                    .resources
                    .iter()
                    .position(|resource| resource.contacts.contains(&piston.near) && resource.contacts.contains(&far))
                    .map(|index| ResourceId::new(index as u32)),
            };
            piston_resources.push(binding);
        }
        let mut resource_buckets = vec![Vec::new(); self.contacts.len()];
        for (index, resource) in self.resources.iter().enumerate() {
            for &contact in resource.contacts.iter() {
                resource_buckets[contact.index()].push(ResourceId::new(index as u32));
            }
        }
        let mut base_piston_buckets = vec![Vec::new(); self.contacts.len()];
        let mut head_piston_buckets = vec![Vec::new(); self.contacts.len()];
        for (index, piston) in self.pistons.iter().enumerate() {
            let id = PistonId::new(index as u32);
            base_piston_buckets[piston.base.index()].push(id);
            head_piston_buckets[piston.near.index()].push(id);
        }

        let mut dust_path_by_wire = vec![None; self.wires.len()];
        for (path_index, path) in self.dust_paths.iter().enumerate() {
            let id = DustPathId::try_from_index(path_index).ok_or(GraphBuildError::TooManyEntries { table: "dust paths" })?;
            for &node in path.wires.iter() {
                let CircuitNodeKind::Wire(wire_index) = self.nodes[node.index()].kind else {
                    unreachable!("dust path references were validated before lowering");
                };
                dust_path_by_wire[wire_index] = Some(id);
            }
        }

        let state = CircuitState {
            components: self.nodes.into_iter().map(|node| node.state).collect::<Vec<_>>().into_boxed_slice(),
            pistons: self.initial_pistons.into_boxed_slice(),
            resources: self.initial_resources.into_boxed_slice(),
        };
        let program = CircuitProgram {
            instant_machine: self.instant_machine,
            instant_proxies: self.instant_proxies.into_boxed_slice(),
            private_resource_owners: private_resource_owners.into_boxed_slice(),
            instant_route_by_route: instant_route_by_route.into_boxed_slice(),
            nodes: nodes.into_boxed_slice(),
            power_links: self.power_links.into_boxed_slice(),
            power_inputs,
            comparator_inputs: self.comparator_inputs.into_boxed_slice(),
            comparator_plan_by_node: comparator_plan_by_node.into_boxed_slice(),
            profiles: self.profiles.into_boxed_slice(),
            profile_blocks,
            guards: self.guards.into_boxed_slice(),
            contacts: self.contacts.into_boxed_slice(),
            routes: self.routes.into_boxed_slice(),
            turbo_callback_indices: self.turbo_callback_indices.into_boxed_slice(),
            wires: self.wires.into_boxed_slice(),
            dust_paths: self.dust_paths.into_boxed_slice(),
            dust_path_by_wire: dust_path_by_wire.into_boxed_slice(),
            sites: self.sites.into_boxed_slice(),
            pistons: self.pistons.into_boxed_slice(),
            resources: self.resources.into_boxed_slice(),
            piston_resources: piston_resources.into_boxed_slice(),
            resource_contacts: ContactCandidates::from_buckets(resource_buckets, "resource contact candidates")?,
            base_pistons: ContactCandidates::from_buckets(base_piston_buckets, "base piston candidates")?,
            head_pistons: ContactCandidates::from_buckets(head_piston_buckets, "head piston candidates")?,
            command_blocks,
            command_plan_by_node: command_plan_by_node.into_boxed_slice(),
            note_blocks,
            note_plan_by_node: note_plan_by_node.into_boxed_slice(),
        };
        Ok((program, state))
    }

    fn validate_inner(&self) -> Result<(), GraphBuildError> {
        // TODO: extract to helper function just like all others.
        for (name, len) in [
            ("nodes", self.nodes.len()),
            ("sites", self.sites.len()),
            ("pistons", self.pistons.len()),
            ("resources", self.resources.len()),
            ("contacts", self.contacts.len()),
            ("profiles", self.profiles.len()),
            ("guards", self.guards.len()),
            ("routes", self.routes.len()),
            ("wires", self.wires.len()),
            ("dust paths", self.dust_paths.len()),
            ("wire relations", self.wire_relations.len()),
            ("instant regions", self.instant_regions.len()),
            ("instant proxies", self.instant_proxies.len()),
        ] {
            if u32::try_from(len).is_err() {
                return Err(GraphBuildError::TooManyEntries { table: name });
            }
        }

        self.validate_nodes()?;
        self.validate_guards()?;
        self.validate_sites_and_wires()?;
        self.validate_turbo_callback_indices()?;
        self.validate_wire_relations()?;
        self.validate_dust_paths()?;
        self.validate_contacts_and_pistons()?;
        self.validate_routes()?;
        self.validate_links()?;
        self.validate_power()?;
        // Certificate inference follows contact variants, so their references must be valid first.
        for (index, wire) in self.wires.iter().enumerate() {
            if let Some(facts) = wire.query_facts
                && facts != WireQueryFacts::infer(self, wire)
            {
                return Err(GraphBuildError::InvalidWireQueryFacts { wire: index });
            }
        }
        self.validate_instant_resets()?;
        self.validate_instant_regions()?;
        if let Some(machine) = &self.instant_machine {
            crate::redpiler::analysis::instant_regions::machine::validate(self, machine)?;
        }
        Ok(())
    }

    fn validate_instant_regions(&self) -> Result<(), GraphBuildError> {
        use crate::redpiler::analysis::instant_regions::{infer_proxies, infer_regions, infer_stable_regions};
        if self.instant_regions.is_empty() && self.instant_proxies.is_empty() {
            return Ok(());
        }
        let monitor = crate::redpiler::TaskMonitor::default();
        let inferred = if self.instant_regions.iter().any(|region| region.stable_inputs) {
            infer_stable_regions(self, &monitor)
        } else {
            infer_regions(self, &monitor)
        };
        let regions = inferred.map_err(|_| GraphBuildError::InvalidInstantRegion { table: "regions", index: 0 })?;
        if self.instant_regions != regions {
            let index = self
                .instant_regions
                .iter()
                .zip(&regions)
                .position(|(actual, expected)| actual != expected)
                .unwrap_or(self.instant_regions.len().min(regions.len()));
            return Err(GraphBuildError::InvalidInstantRegion { table: "regions", index });
        }
        if !self.instant_proxies.is_empty() {
            let proxies =
                infer_proxies(self, &regions, &monitor).map_err(|_| GraphBuildError::InvalidInstantRegion { table: "proxies", index: 0 })?;
            if self.instant_proxies != proxies {
                let index = self
                    .instant_proxies
                    .iter()
                    .zip(&proxies)
                    .position(|(actual, expected)| actual != expected)
                    .unwrap_or(self.instant_proxies.len().min(proxies.len()));
                return Err(GraphBuildError::InvalidInstantRegion { table: "proxies", index });
            }
        }
        Ok(())
    }

    fn validate_instant_resets(&self) -> Result<(), GraphBuildError> {
        if !self.wires.iter().any(|wire| wire.instant_reset.is_some()) {
            return Ok(());
        }
        let mut inferred = vec![None; self.wires.len()];
        for (wire, plan) in crate::redpiler::passes::simplify_instant_resets::infer_instant_resets(self) {
            inferred[wire] = Some(plan);
        }
        for (index, wire) in self.wires.iter().enumerate() {
            if let Some(plan) = wire.instant_reset
                && inferred[index] != Some(plan)
            {
                return Err(GraphBuildError::InvalidInstantReset { wire: index });
            }
        }
        Ok(())
    }

    fn validate_turbo_callback_indices(&self) -> Result<(), GraphBuildError> {
        if self.turbo_callback_indices.is_empty() {
            return Ok(());
        }
        if self.turbo_callback_indices.len() != self.wires.len() {
            return Err(GraphBuildError::StateTableLength {
                table: "Turbo callback indices",
                descriptions: self.wires.len(),
                states: self.turbo_callback_indices.len(),
            });
        }
        for (wire_index, index) in self.turbo_callback_indices.iter().enumerate() {
            let Some(index) = index else {
                continue;
            };
            let wire = &self.wires[wire_index];
            let route_id = wire.turbo_route.ok_or(GraphBuildError::InvalidTurboCallbackIndex {
                wire: wire_index,
                reason: "indexed wire has no Turbo route",
            })?;
            let route = self.routes.get(route_id.index()).ok_or(GraphBuildError::InvalidTurboCallbackIndex {
                wire: wire_index,
                reason: "indexed Turbo route is missing",
            })?;
            let deliveries = &route.deliveries;
            let route_len = u32::try_from(deliveries.len()).map_err(|_| GraphBuildError::InvalidTurboCallbackIndex {
                wire: wire_index,
                reason: "Turbo route length exceeds indexed range",
            })?;
            if index.offsets[0] != 0
                || index.offsets[24] != route_len
                || index
                    .offsets
                    .windows(2)
                    .any(|range| range[0] > range[1] || usize::try_from(range[1]).unwrap() > deliveries.len())
            {
                return Err(GraphBuildError::InvalidTurboCallbackIndex {
                    wire: wire_index,
                    reason: "offsets are not ordered route bounds",
                });
            }
            for slot in 0..24 {
                let target = wire.canonical_neighbors[slot];
                if wire.canonical_neighbors[..slot].contains(&target) {
                    return Err(GraphBuildError::InvalidTurboCallbackIndex {
                        wire: wire_index,
                        reason: "canonical targets are repeated",
                    });
                }
                for delivery in &deliveries[index.offsets[slot] as usize..index.offsets[slot + 1] as usize] {
                    if delivery.target != target {
                        return Err(GraphBuildError::InvalidTurboCallbackIndex {
                            wire: wire_index,
                            reason: "slot range contains a different target",
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_dust_paths(&self) -> Result<(), GraphBuildError> {
        let mut assigned = vec![false; self.wires.len()];
        for (path_index, path) in self.dust_paths.iter().enumerate() {
            if !(2..=3).contains(&path.wires.len()) {
                return Err(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "path length must be two or three wires",
                });
            }
            check_id("nodes", path.input_node.index(), self.nodes.len())?;
            let input_site = self.sites.get(path.input_site.index()).ok_or(GraphBuildError::InvalidReference {
                table: "sites",
                index: path.input_site.index(),
                len: self.sites.len(),
            })?;
            if input_site.source != SiteSource::Component(path.input_node)
                || !matches!(self.nodes[path.input_node.index()].kind, CircuitNodeKind::Electrical(_))
                || self.nodes[path.input_node.index()].raw_state.is_none()
            {
                return Err(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "input site is not owned by an electrical component",
                });
            }
            check_id("nodes", path.wires[0].index(), self.nodes.len())?;
            let first_position = self.nodes[path.wires[0].index()].position.ok_or(GraphBuildError::InvalidDustPath {
                path: path_index,
                reason: "wire positions are required",
            })?;
            if input_site.position != first_position.offset(path.input_face) {
                return Err(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "input site is not directly adjacent on the declared face",
                });
            }
            if !matches!(path.input_face, BlockFace::North | BlockFace::South | BlockFace::East | BlockFace::West)
                || !self.power_links.iter().any(|link| {
                    link.from == path.input_site
                        && link.from_port == PowerPort::Weak(path.input_face)
                        && link.to
                            == (PowerTarget::Input {
                                node: path.wires[0],
                                port: InputPort::Main,
                            })
                        && link.face == path.input_face
                        && link.attenuation == 0
                        && self.guards.get(link.guard.index()) == Some(&Guard::Always)
                })
            {
                return Err(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "direct ingress power binding is missing or non-horizontal",
                });
            }
            let mut previous: Option<BlockPos> = None;
            let mut axis = None;
            for &node in path.wires.iter() {
                check_id("nodes", node.index(), self.nodes.len())?;
                let CircuitNodeKind::Wire(wire_index) = self.nodes[node.index()].kind else {
                    return Err(GraphBuildError::InvalidDustPath {
                        path: path_index,
                        reason: "path contains a non-wire node",
                    });
                };
                let wire = self.wires.get(wire_index).ok_or(GraphBuildError::InvalidReference {
                    table: "wires",
                    index: wire_index,
                    len: self.wires.len(),
                })?;
                if wire.node != node || assigned[wire_index] {
                    return Err(GraphBuildError::InvalidDustPath {
                        path: path_index,
                        reason: "wire is mismatched or assigned to multiple paths",
                    });
                }
                assigned[wire_index] = true;
                let position = self.nodes[node.index()].position.ok_or(GraphBuildError::InvalidDustPath {
                    path: path_index,
                    reason: "wire positions are required",
                })?;
                if let Some(previous) = previous {
                    let dx = position.x - previous.x;
                    let dz = position.z - previous.z;
                    let this_axis = if position.y == previous.y && dx.abs() == 1 && dz == 0 {
                        Some(0)
                    } else if position.y == previous.y && dz.abs() == 1 && dx == 0 {
                        Some(1)
                    } else {
                        None
                    };
                    if this_axis.is_none() || axis.is_some_and(|axis| Some(axis) != this_axis) {
                        return Err(GraphBuildError::InvalidDustPath {
                            path: path_index,
                            reason: "wires are not a straight horizontal path",
                        });
                    }
                    axis = this_axis;
                }
                previous = Some(position);
            }
        }
        Ok(())
    }

    fn validate_power(&self) -> Result<(), GraphBuildError> {
        for link in &self.power_links {
            check_id("sites", link.from.index(), self.sites.len())?;
            check_id("guards", link.guard.index(), self.guards.len())?;
            if link.attenuation > 15 {
                return Err(GraphBuildError::InvalidPowerAttenuation {
                    from: link.from,
                    attenuation: link.attenuation,
                });
            }
            match link.to {
                PowerTarget::Input { node, .. } => check_id("nodes", node.index(), self.nodes.len())?,
                PowerTarget::Conductor { site, .. } => check_id("sites", site.index(), self.sites.len())?,
            }
        }
        for plan in &self.comparator_inputs {
            check_id("nodes", plan.node.index(), self.nodes.len())?;
            check_id("sites", plan.rear.index(), self.sites.len())?;
            if !matches!(
                self.nodes[plan.node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Comparator { .. })
            ) {
                return Err(GraphBuildError::InvalidComponentBinding { node: plan.node });
            }
            let validate_override = |source| match source {
                ComparatorOverride::Literal(strength) if strength > 15 => Err(GraphBuildError::InvalidStrength { node: plan.node, strength }),
                ComparatorOverride::LiveComponentOutput(node) => check_id("nodes", node.index(), self.nodes.len()),
                _ => Ok(()),
            };
            if let Some(source) = plan.direct_override {
                validate_override(source)?;
            }
            if let Some((site, source, guard)) = plan.far_override {
                check_id("sites", site.index(), self.sites.len())?;
                check_id("guards", guard.index(), self.guards.len())?;
                validate_override(source)?;
            }
            if plan.far_threshold > 15 {
                return Err(GraphBuildError::InvalidStrength {
                    node: plan.node,
                    strength: plan.far_threshold,
                });
            }
        }
        Ok(())
    }

    fn validate_nodes(&self) -> Result<(), GraphBuildError> {
        for (index, node) in self.nodes.iter().enumerate() {
            let id = NodeId::try_from_index(index).expect("node count was checked");
            let valid_state = match (&node.kind, &node.state) {
                (CircuitNodeKind::Electrical(ty), ComponentState::Electrical(state)) => {
                    if node.raw_state.is_none() {
                        return Err(GraphBuildError::MissingComponentState { node: id });
                    }
                    if matches!(ty, NodeType::Wire | NodeType::Observer { .. }) {
                        return Err(GraphBuildError::UseSpecializedNode { node: id });
                    }
                    validate_strength(
                        id,
                        state.output_strength,
                        match ty {
                            NodeType::Comparator { far_input, .. } => *far_input,
                            _ => None,
                        },
                    )?;
                    true
                }
                (CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. }, ComponentState::Electrical(state)) => {
                    if node.raw_state.is_none() {
                        return Err(GraphBuildError::MissingComponentState { node: id });
                    }
                    check_id("sites", watched.index(), self.sites.len())?;
                    validate_strength(id, state.output_strength, None)?;
                    if let CircuitNodeKind::BuiltinReset { facing, .. } = node.kind {
                        if node.output_route.is_some()
                            || !matches!(node.raw_state.map(Block::from_id), Some(Block::Observer { observer }) if BlockFace::from(observer.facing) == facing)
                        {
                            return Err(GraphBuildError::NodeKindMismatch { node: id });
                        }
                    }
                    true
                }
                (CircuitNodeKind::Wire(plan), ComponentState::Wire(state)) => {
                    if node.raw_state.is_none() {
                        return Err(GraphBuildError::MissingComponentState { node: id });
                    }
                    if state.power > 15 {
                        return Err(GraphBuildError::InvalidStrength {
                            node: id,
                            strength: state.power,
                        });
                    }
                    let wire = self.wires.get(*plan).ok_or(GraphBuildError::InvalidReference {
                        table: "wires",
                        index: *plan,
                        len: self.wires.len(),
                    })?;
                    if wire.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                (CircuitNodeKind::Piston(piston), ComponentState::Specialized) => {
                    let spec = self.pistons.get(piston.index()).ok_or(GraphBuildError::InvalidReference {
                        table: "pistons",
                        index: piston.index(),
                        len: self.pistons.len(),
                    })?;
                    if spec.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                (CircuitNodeKind::Contact(contact), ComponentState::Specialized) => {
                    let spec = self.contacts.get(contact.index()).ok_or(GraphBuildError::InvalidReference {
                        table: "contacts",
                        index: contact.index(),
                        len: self.contacts.len(),
                    })?;
                    if spec.node != id {
                        return Err(GraphBuildError::NodeKindMismatch { node: id });
                    }
                    true
                }
                _ => false,
            };
            if !valid_state {
                return Err(GraphBuildError::InvalidNodeState { node: id });
            }
            if let Some(route) = node.output_route {
                check_id("routes", route.index(), self.routes.len())?;
                if self.routes[route.index()].source != id {
                    return Err(GraphBuildError::RouteSourceMismatch { route, node: id });
                }
            }
        }
        Ok(())
    }

    fn validate_guards(&self) -> Result<(), GraphBuildError> {
        for guard in &self.guards {
            match guard {
                Guard::Always => {}
                Guard::State(test) => self.validate_state_test(*test)?,
                Guard::Site { site, test, .. } => {
                    check_id("sites", site.index(), self.sites.len())?;
                    if let SiteTest::Profile(profile) = test {
                        check_id("profiles", profile.index(), self.profiles.len())?;
                    }
                }
                Guard::Not(other) => check_id("guards", other.index(), self.guards.len())?,
                Guard::All(others) | Guard::Any(others) => {
                    for other in others.iter() {
                        check_id("guards", other.index(), self.guards.len())?;
                    }
                }
            }
        }

        let mut marks = vec![0u8; self.guards.len()];
        for root_index in 0..self.guards.len() {
            if marks[root_index] != 0 {
                continue;
            }
            let root = GuardId::try_from_index(root_index).expect("guard count was checked");
            let mut stack = vec![(root, false)];
            while let Some((id, leaving)) = stack.pop() {
                let guard_count = marks.len();
                let mark = marks.get_mut(id.index()).ok_or(GraphBuildError::InvalidReference {
                    table: "guards",
                    index: id.index(),
                    len: guard_count,
                })?;
                if leaving {
                    *mark = 2;
                    continue;
                }
                if *mark == 1 {
                    return Err(GraphBuildError::CyclicGuard { guard: id });
                }
                if *mark == 2 {
                    continue;
                }
                *mark = 1;
                stack.push((id, true));
                if let Some(guard) = self.guards.get(id.index()) {
                    for child in guard_children(guard).iter().rev() {
                        if marks.get(child.index()) == Some(&1) {
                            return Err(GraphBuildError::CyclicGuard { guard: *child });
                        }
                        if marks.get(child.index()) == Some(&0) {
                            stack.push((*child, false));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_state_test(&self, test: StateTest) -> Result<(), GraphBuildError> {
        match test {
            StateTest::PistonExtended(piston) | StateTest::PistonHeadPresent(piston) | StateTest::PistonAvailability { piston, .. } => {
                check_id("pistons", piston.index(), self.pistons.len())?;
            }
            StateTest::ResourceAt { resource, contact } => {
                check_id("contacts", contact.index(), self.contacts.len())?;
                let spec = self.resources.get(resource.index()).ok_or(GraphBuildError::InvalidReference {
                    table: "resources",
                    index: resource.index(),
                    len: self.resources.len(),
                })?;
                if !spec.contacts.contains(&contact) {
                    return Err(GraphBuildError::InvalidResourceSelection { resource, contact });
                }
            }
            StateTest::ResourceEnabled(resource) => {
                check_id("resources", resource.index(), self.resources.len())?;
            }
        }
        Ok(())
    }

    fn validate_sites_and_wires(&self) -> Result<(), GraphBuildError> {
        for site in &self.sites {
            match site.source {
                SiteSource::Component(node) => self.validate_component_binding(node)?,
                SiteSource::Contact(contact) => {
                    check_id("contacts", contact.index(), self.contacts.len())?;
                }
                SiteSource::Constant(profile) => {
                    check_id("profiles", profile.index(), self.profiles.len())?;
                }
            }
        }

        let mut hop_root_counts = vec![0u32; self.nodes.len()];
        if self.wires.iter().any(|wire| wire.hop_root.is_some()) {
            for site in &self.sites {
                if let SiteSource::Component(node) = site.source {
                    hop_root_counts[node.index()] += 1;
                }
            }
        }
        for (index, wire) in self.wires.iter().enumerate() {
            let node = self.nodes.get(wire.node.index()).ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: wire.node.index(),
                len: self.nodes.len(),
            })?;
            if node.kind != CircuitNodeKind::Wire(index) {
                return Err(GraphBuildError::NodeKindMismatch { node: wire.node });
            }
            if let Some(root) = wire.hop_root
                && (hop_root_counts[wire.node.index()] != 1
                    || !self
                        .sites
                        .get(root.index())
                        .is_some_and(|site| site.source == SiteSource::Component(wire.node)))
            {
                return Err(GraphBuildError::InvalidWireHopRoot { wire: index });
            }
            for site in wire.canonical_neighbors {
                check_id("sites", site.index(), self.sites.len())?;
            }
            for route in wire.shape_route.into_iter().chain(wire.turbo_route) {
                check_id("routes", route.index(), self.routes.len())?;
                if self.routes[route.index()].source != wire.node {
                    return Err(GraphBuildError::RouteSourceMismatch { route, node: wire.node });
                }
            }
        }
        Ok(())
    }

    fn validate_wire_relations(&self) -> Result<(), GraphBuildError> {
        for relation in &self.wire_relations {
            let node = self.nodes.get(relation.from.index()).ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: relation.from.index(),
                len: self.nodes.len(),
            })?;
            if !matches!(node.kind, CircuitNodeKind::Wire(_)) {
                return Err(GraphBuildError::NodeKindMismatch { node: relation.from });
            }
            check_id("sites", relation.to.index(), self.sites.len())?;
            check_id("guards", relation.guard.index(), self.guards.len())?;
        }
        Ok(())
    }

    fn validate_contacts_and_pistons(&self) -> Result<(), GraphBuildError> {
        for (index, contact) in self.contacts.iter().enumerate() {
            let id = ContactId::try_from_index(index).expect("contact count was checked");
            let node = self.nodes.get(contact.node.index()).ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: contact.node.index(),
                len: self.nodes.len(),
            })?;
            if node.kind != CircuitNodeKind::Contact(id) {
                return Err(GraphBuildError::NodeKindMismatch { node: contact.node });
            }
            if contact.variants.is_empty() {
                return Err(GraphBuildError::EmptyContact { contact: id });
            }
            for variant in contact.variants.iter() {
                check_id("guards", variant.when.index(), self.guards.len())?;
                self.validate_contact_guard(id, variant.when)?;
                check_id("profiles", variant.profile.index(), self.profiles.len())?;
                if let Some(receiver) = variant.receiver {
                    self.validate_receiver(receiver)?;
                }
            }
        }

        for (index, piston) in self.pistons.iter().enumerate() {
            let id = PistonId::try_from_index(index).expect("piston count was checked");
            let node = self.nodes.get(piston.node.index()).ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: piston.node.index(),
                len: self.nodes.len(),
            })?;
            if node.kind != CircuitNodeKind::Piston(id) {
                return Err(GraphBuildError::NodeKindMismatch { node: piston.node });
            }
            if node.position != Some(piston.position) {
                return Err(GraphBuildError::NodeKindMismatch { node: piston.node });
            }
            check_id("contacts", piston.base.index(), self.contacts.len())?;
            check_id("contacts", piston.near.index(), self.contacts.len())?;
            check_id("contacts", piston.far.index(), self.contacts.len())?;
            if let PistonLane::SingleResource { far } = piston.lane {
                check_id("contacts", far.index(), self.contacts.len())?;
                if far != piston.far {
                    return Err(GraphBuildError::PistonFarContactMismatch { node: piston.node });
                }
            }
            self.validate_piston_routes(piston.node, piston.lane, piston.routes)?;
        }

        if self.initial_pistons.len() != self.pistons.len() {
            return Err(GraphBuildError::StateTableLength {
                table: "pistons",
                descriptions: self.pistons.len(),
                states: self.initial_pistons.len(),
            });
        }
        if self.initial_resources.len() != self.resources.len() {
            return Err(GraphBuildError::StateTableLength {
                table: "resources",
                descriptions: self.resources.len(),
                states: self.initial_resources.len(),
            });
        }

        let mut contact_owners = HashMap::new();
        for (index, resource) in self.resources.iter().enumerate() {
            let id = ResourceId::try_from_index(index).expect("resource count was checked");
            check_id("profiles", resource.material.index(), self.profiles.len())?;
            if resource.contacts.is_empty() {
                return Err(GraphBuildError::EmptyResource { resource: id });
            }
            for contact in resource.contacts.iter() {
                check_id("contacts", contact.index(), self.contacts.len())?;
                if let Some(first) = contact_owners.insert(*contact, id) {
                    return Err(GraphBuildError::ResourcesShareContact {
                        first,
                        second: id,
                        contact: *contact,
                    });
                }
            }
            let state = self.initial_resources.get(index).copied().expect("resource state count was validated");
            if !resource.contacts.contains(&state.at) {
                return Err(GraphBuildError::InvalidResourceSelection {
                    resource: id,
                    contact: state.at,
                });
            }
        }

        let mut selected_contacts = HashMap::with_capacity(self.initial_resources.len());
        for (index, state) in self.initial_resources.iter().enumerate() {
            let resource = ResourceId::try_from_index(index).expect("resource count was checked");
            if let Some(first) = selected_contacts.insert(state.at, resource) {
                return Err(GraphBuildError::ResourcesShareContact {
                    first,
                    second: resource,
                    contact: state.at,
                });
            }
        }
        Ok(())
    }

    fn validate_contact_guard(&self, contact: ContactId, root: GuardId) -> Result<(), GraphBuildError> {
        let mut visited = rustc_hash::FxHashSet::default();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let guard = &self.guards[id.index()];
            if let Guard::Site { site, .. } = guard {
                return Err(GraphBuildError::ContactGuardReadsSite {
                    contact,
                    guard: id,
                    site: *site,
                });
            }
            pending.extend(guard_children(guard).iter().copied());
        }
        Ok(())
    }

    fn validate_receiver(&self, receiver: CallbackReceiver) -> Result<(), GraphBuildError> {
        match receiver {
            CallbackReceiver::Component(node) => self.validate_component_binding(node),
            CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => check_id("pistons", piston.index(), self.pistons.len()),
            CallbackReceiver::Contact(contact) => check_id("contacts", contact.index(), self.contacts.len()),
        }
    }

    fn validate_component_binding(&self, node: NodeId) -> Result<(), GraphBuildError> {
        let kind = &self
            .nodes
            .get(node.index())
            .ok_or(GraphBuildError::InvalidReference {
                table: "nodes",
                index: node.index(),
                len: self.nodes.len(),
            })?
            .kind;
        if matches!(
            kind,
            CircuitNodeKind::Electrical(_) | CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Wire(_)
        ) {
            Ok(())
        } else {
            Err(GraphBuildError::InvalidComponentBinding { node })
        }
    }

    fn validate_piston_routes(&self, source: NodeId, lane: PistonLane, routes: PistonRoutes) -> Result<(), GraphBuildError> {
        let has_resource_routes = routes.resource_restored_near.is_some() && routes.resource_restored_far.is_some();
        if matches!(lane, PistonLane::SingleResource { .. }) != has_resource_routes
            || routes.resource_restored_near.is_some() != routes.resource_restored_far.is_some()
        {
            return Err(GraphBuildError::PistonResourceRestorationMismatch { node: source });
        }
        for route in [
            Some(routes.base_changed),
            Some(routes.near_changed),
            routes.far_changed,
            routes.far_shape_changed,
            Some(routes.base_restored),
            Some(routes.head_restored),
            routes.resource_restored_near,
            routes.resource_restored_far,
        ]
        .into_iter()
        .flatten()
        {
            check_id("routes", route.index(), self.routes.len())?;
            if self.routes[route.index()].source != source {
                return Err(GraphBuildError::RouteSourceMismatch { route, node: source });
            }
        }
        Ok(())
    }

    fn validate_routes(&self) -> Result<(), GraphBuildError> {
        for (route_index, route) in self.routes.iter().enumerate() {
            let route_id = RouteId::try_from_index(route_index).expect("route count was checked");
            check_id("nodes", route.source.index(), self.nodes.len())?;
            let mut previous_sequence = 0;
            for (delivery_index, delivery) in route.deliveries.iter().enumerate() {
                check_id("sites", delivery.target.index(), self.sites.len())?;
                check_id("guards", delivery.guard.index(), self.guards.len())?;
                if delivery_index > 0 && delivery.sequence < previous_sequence {
                    return Err(GraphBuildError::UnorderedRoute { route: route_id });
                }
                previous_sequence = delivery.sequence;
                if let Some(receiver) = delivery.receiver {
                    self.validate_receiver(receiver)?;
                    self.validate_delivery_receiver(route_id, delivery, receiver)?;
                }
            }
        }
        Ok(())
    }

    fn receiver_node(&self, receiver: CallbackReceiver) -> Result<NodeId, GraphBuildError> {
        match receiver {
            CallbackReceiver::Component(node) => Ok(node),
            CallbackReceiver::PistonBase(id) | CallbackReceiver::PistonHead(id) => {
                self.pistons
                    .get(id.index())
                    .map(|piston| piston.node)
                    .ok_or(GraphBuildError::InvalidReference {
                        table: "pistons",
                        index: id.index(),
                        len: self.pistons.len(),
                    })
            }
            CallbackReceiver::Contact(id) => self
                .contacts
                .get(id.index())
                .map(|contact| contact.node)
                .ok_or(GraphBuildError::InvalidReference {
                    table: "contacts",
                    index: id.index(),
                    len: self.contacts.len(),
                }),
        }
    }

    fn validate_delivery_receiver(&self, route: RouteId, delivery: &RouteDelivery, receiver: CallbackReceiver) -> Result<(), GraphBuildError> {
        let site = self.sites[delivery.target.index()];
        let valid = match (delivery.kind, site.source, receiver) {
            (DeliveryKind::Shape { .. }, SiteSource::Contact(contact), CallbackReceiver::Contact(target)) => {
                contact == target && self.contact_guard_matches(delivery, contact, None)
            }
            (DeliveryKind::Shape { .. }, SiteSource::Component(node), CallbackReceiver::Component(target)) => node == target,
            (DeliveryKind::Update { .. }, SiteSource::Component(node), CallbackReceiver::Component(target)) => node == target,
            (DeliveryKind::Update { .. }, SiteSource::Contact(contact), receiver) => self.contacts[contact.index()]
                .variants
                .iter()
                .any(|variant| variant.receiver == Some(receiver) && self.contact_guard_matches(delivery, contact, Some(variant.profile))),
            (_, SiteSource::Constant(_), _) => false,
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(GraphBuildError::CallbackEdgeMismatch {
                route,
                sequence: delivery.sequence,
            })
        }
    }

    fn contact_guard_matches(&self, delivery: &RouteDelivery, contact: ContactId, profile: Option<ProfileId>) -> bool {
        match delivery.view {
            OperandView::Live => self.contacts[contact.index()]
                .variants
                .iter()
                .filter(|variant| profile.map_or(true, |profile| variant.profile == profile))
                .any(|variant| matches!(self.guards[variant.when.index()], Guard::Always) || self.guard_implies(delivery.guard, variant.when)),
            OperandView::Captured => self.contacts[contact.index()]
                .variants
                .iter()
                .filter(|variant| profile.map_or(true, |profile| variant.profile == profile))
                .any(|variant| self.guard_implies_site_profile(delivery.guard, delivery.target, variant.profile)),
        }
    }

    fn guard_implies(&self, root: GuardId, expected: GuardId) -> bool {
        if root == expected {
            return true;
        }
        match &self.guards[root.index()] {
            Guard::All(children) => children.iter().any(|child| self.guard_implies(*child, expected)),
            Guard::Any(children) => !children.is_empty() && children.iter().all(|child| self.guard_implies(*child, expected)),
            _ => false,
        }
    }

    fn guard_implies_site_profile(&self, root: GuardId, site: SiteId, profile: ProfileId) -> bool {
        if matches!(
            self.guards[root.index()],
                Guard::Site {
                    site: candidate_site,
                    view: OperandView::Captured,
                    test: SiteTest::Profile(candidate_profile),
                } if candidate_site == site && candidate_profile == profile
        ) {
            return true;
        }
        match &self.guards[root.index()] {
            Guard::All(children) => children.iter().any(|child| self.guard_implies_site_profile(*child, site, profile)),
            Guard::Any(children) => !children.is_empty() && children.iter().all(|child| self.guard_implies_site_profile(*child, site, profile)),
            _ => false,
        }
    }

    fn validate_links(&self) -> Result<(), GraphBuildError> {
        let mut callback_edges = HashMap::<(CallbackEdgeRef, NodeId, NodeId, LinkKind), usize>::new();
        for link in &self.links {
            check_id("nodes", link.from.index(), self.nodes.len())?;
            check_id("nodes", link.to.index(), self.nodes.len())?;
            let guard = match link.kind {
                LinkKind::Update { guard, .. } | LinkKind::Shape { guard, .. } => guard,
            };
            check_id("guards", guard.index(), self.guards.len())?;
            let reference = link.callback;
            let route = self.routes.get(reference.route.index()).ok_or(GraphBuildError::InvalidReference {
                table: "routes",
                index: reference.route.index(),
                len: self.routes.len(),
            })?;
            if route.source != link.from {
                return Err(GraphBuildError::CallbackEdgeMismatch {
                    route: reference.route,
                    sequence: reference.sequence,
                });
            }
            *callback_edges.entry((reference, link.from, link.to, link.kind)).or_default() += 1;
        }

        for (route_index, route) in self.routes.iter().enumerate() {
            let route_id = RouteId::try_from_index(route_index).expect("route count was checked");
            for delivery in route.deliveries.iter() {
                let Some(receiver) = delivery.receiver else {
                    continue;
                };
                let to = self.receiver_node(receiver)?;
                let key = (
                    CallbackEdgeRef {
                        route: route_id,
                        sequence: delivery.sequence,
                    },
                    route.source,
                    to,
                    delivery.link_kind(),
                );
                let Some(count) = callback_edges.get_mut(&key) else {
                    return Err(GraphBuildError::CallbackEdgeMismatch {
                        route: route_id,
                        sequence: delivery.sequence,
                    });
                };
                let Some(remaining) = count.checked_sub(1) else {
                    return Err(GraphBuildError::CallbackEdgeMismatch {
                        route: route_id,
                        sequence: delivery.sequence,
                    });
                };
                *count = remaining;
            }
        }
        if let Some(((reference, _, _, _), _)) = callback_edges.iter().find(|(_, count)| **count != 0) {
            return Err(GraphBuildError::CallbackEdgeMismatch {
                route: reference.route,
                sequence: reference.sequence,
            });
        }
        Ok(())
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

fn guard_children(guard: &Guard) -> &[GuardId] {
    match guard {
        Guard::Not(id) => std::slice::from_ref(id),
        Guard::All(ids) | Guard::Any(ids) => ids,
        _ => &[],
    }
}

fn validate_strength(node: NodeId, output: u8, far_input: Option<u8>) -> Result<(), GraphBuildError> {
    for strength in std::iter::once(output).chain(far_input) {
        if strength > 15 {
            return Err(GraphBuildError::InvalidStrength { node, strength });
        }
    }
    Ok(())
}

fn check_id(table: &'static str, index: usize, len: usize) -> Result<(), GraphBuildError> {
    if index >= len {
        Err(GraphBuildError::InvalidReference { table, index, len })
    } else {
        Ok(())
    }
}

#[derive(Debug)]
/// Structural validation failures encountered while building or lowering a graph.
pub(crate) enum GraphBuildError {
    InvalidInstantRegion {
        /// This names the derived table whose witness no longer agrees with the graph.
        table: &'static str,
        /// This identifies the first stale or missing certificate in that table.
        index: usize,
    },
    InvalidInstantMachine {
        /// This identifies the normalized snapshot whose transition proof failed.
        state: usize,
        /// This identifies tick column zero or the corresponding lever-toggle column.
        action: usize,
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
            Self::InvalidInstantRegion { table, index } => write!(f, "instant {table}[{index}] has a stale or invalid certificate"),
            Self::InvalidInstantMachine { state, action } => {
                write!(f, "instant machine state {state}, action {action} has an invalid transition certificate")
            }
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
/// Immutable tables used by the graph executor after lowering.
pub(crate) struct CircuitProgram {
    /// This owns certified closed-selection transitions; unsupported states retain general execution.
    instant_machine: Option<std::sync::Arc<InstantMachine>>,
    /// These private-cell programs reuse the existing actor, resource, and clock state.
    instant_proxies: Box<[InstantProxy]>,
    /// This certifies that no foreign base, head, or resource can reach a private payload's contacts.
    private_resource_owners: Box<[Option<PistonId>]>,
    /// This binds a route directly to its proxy and ordered instruction sequence.
    instant_route_by_route: Box<[Option<(u32, u32)>]>,
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
    pub(crate) fn instant_machine(&self) -> Option<&std::sync::Arc<InstantMachine>> {
        self.instant_machine.as_ref()
    }
    pub(crate) fn private_resource_owner(&self, resource: ResourceId) -> Option<PistonId> {
        self.private_resource_owners[resource.index()]
    }
    pub(crate) fn instant_proxies(&self) -> &[InstantProxy] {
        &self.instant_proxies
    }

    pub(crate) fn instant_route_binding(&self, route: RouteId) -> Option<(u32, u32)> {
        self.instant_route_by_route[route.index()]
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
            turbo_callback_indices: vec![None],
            wires: vec![WirePlan {
                node: NodeId::new(5),
                canonical_neighbors: [site; 24],
                shape_route: None,
                turbo_route: None,
                query_facts: None,
                instant_reset: None,
                hop_root: None,
            }],
            dust_paths: vec![],
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
            instant_regions: Vec::new(),
            instant_proxies: Vec::new(),
            instant_machine: None,
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
