//! Run the compiled circuit.
//!
//! [`GraphBackend`] holds the program, circuit state, clock, and pending work.
//! Power reads and callbacks use these tables. Commands get world access through
//! a separate host. Flush writes display changes; reset returns the circuit to
//! the interpreter. See the [redpiler overview](crate::redpiler).

#[cfg(test)]
pub(crate) mod accepted_boundary_probe;
#[cfg(test)]
#[path = "graph/tests/chungus_census.rs"]
mod chungus_census;
pub(crate) mod classic;
mod coverage;
mod cpu_kernels;
#[cfg(test)]
#[path = "graph/tests/cpu_power_queries.rs"]
mod cpu_power_query_tests;
mod drives;
#[cfg(test)]
#[path = "graph/tests/event_boundary_probes.rs"]
mod event_boundary_probes;
#[cfg(test)]
mod firing_probe;
mod fixed_classic;
mod instant;
#[cfg(test)]
#[path = "graph/tests/instant_cpu.rs"]
mod instant_cpu_tests;
#[cfg(test)]
#[path = "graph/tests/instant_firing_reset.rs"]
mod instant_firing_reset_tests;
mod instant_kernel;
mod instant_proxy;
#[cfg(test)]
#[path = "graph/tests/logical_bud_runtime.rs"]
mod logical_bud_runtime_tests;
mod logical_clock;
mod logical_device;
#[cfg(test)]
#[path = "graph/tests/logical_device_author_profiles.rs"]
mod logical_device_author_profile_tests;
#[cfg(test)]
#[path = "graph/tests/logical_device_counter.rs"]
mod logical_device_counter_tests;
#[cfg(test)]
#[path = "graph/tests/logical_device_frame.rs"]
mod logical_device_frame_tests;
#[cfg(test)]
#[path = "graph/tests/logical_device_islands.rs"]
mod logical_device_island_tests;
#[cfg(test)]
#[path = "graph/tests/logical_device_saved_plot.rs"]
mod logical_device_saved_plot_tests;
mod logical_firing;
#[cfg(test)]
#[path = "graph/tests/memory_cells.rs"]
mod memory_cell_tests;
#[cfg(test)]
#[path = "graph/tests/mixed_classic.rs"]
mod mixed_classic_tests;
#[cfg(test)]
#[path = "graph/tests/piston_sample_census.rs"]
mod piston_sample_census;
mod pistons;
mod power;
mod projection;
mod reset;
mod settled_inputs;
#[cfg(test)]
#[path = "graph/tests/settled_timing.rs"]
mod settled_timing;
mod signal_channels;
mod sites;
mod stable_instant_logic;
mod transitions;
pub(crate) mod transport;
#[cfg(test)]
#[path = "graph/tests/unchecked_classic_anpu.rs"]
mod unchecked_classic_anpu;
mod wire;

use pistons::{DeferredPistonEvent, PistonRequest};
use sites::ResolvedSiteView;

use super::TickScheduler;
use super::graph_trace::graph_trace;
use crate::redpiler::GraphExecutionCounts;
use crate::redpiler::TaskMonitor;
#[cfg(test)]
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::analysis::{AnalysisError, callbacks, electrical, geometry};
use crate::redpiler::components::{NodeState, NodeType};
use crate::redpiler::passes;
#[cfg(test)]
use crate::redpiler::passes::rich::PreparationError;
use crate::redpiler::program_graph::*;
use crate::world::{BlockAction, World};
use mchprs_blocks::block_entities::{BlockEntity, ContainerType, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, ComparatorMode, RedstonePiston, RedstoneWire, RedstoneWireSide};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{PistonEvent, TickEntry, TickPriority};
use rustc_hash::{FxHashMap, FxHashSet};
#[cfg(test)]
use std::cell::Cell;
use std::collections::VecDeque;
use std::fmt;
#[cfg(test)]
use std::ops::{Deref, DerefMut};

#[cfg(test)]
macro_rules! graph_test_trace {
    ($backend:expr, $value:expr) => {
        if $backend.test_recording {
            $backend.test_recorder.trace.push($value);
        }
    };
}
#[cfg(test)]
pub(crate) use graph_test_trace;
#[cfg(test)]
use std::sync::Arc;

const SCHEDULER_BUCKETS: usize = super::queue::NUM_QUEUES;

#[derive(Debug)]
pub(crate) enum GraphBackendError {
    Cancelled,
    Analysis(AnalysisError),
    Geometry(geometry::GeometryError),
    Electrical(electrical::ElectricalError),
    Callbacks(callbacks::CallbackError),
    PassPipeline(passes::PassPipelineError),
    Graph(GraphBuildError),
    MissingBlockState { node: NodeId },
    MissingComponentSite { node: NodeId },
    MissingNoteBlockSite { node: NodeId, position: BlockPos },
    IncompleteWireNeighborhood { node: NodeId, position: BlockPos },
    TickOutsideScheduler { delay: u32 },
    InvalidZeroDelayPriority { priority: TickPriority },
    UnsupportedTickTarget { position: BlockPos, block_type: u32 },
    UnknownTickTarget { position: BlockPos },
    UnsupportedComponent { position: Option<BlockPos>, kind: &'static str },
}

impl fmt::Display for GraphBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("graph preparation cancelled"),
            Self::Analysis(error) => error.fmt(f),
            Self::Geometry(error) => error.fmt(f),
            Self::Electrical(error) => error.fmt(f),
            Self::Callbacks(error) => error.fmt(f),
            Self::PassPipeline(error) => error.fmt(f),
            Self::Graph(error) => error.fmt(f),
            Self::MissingBlockState { node } => {
                write!(f, "component node {} has no original block state", node.index())
            }
            Self::MissingComponentSite { node } => {
                write!(f, "wire node {} has no compiled component site", node.index())
            }
            Self::MissingNoteBlockSite { node, position } => write!(
                f,
                "noteblock node {} at {position:?} is missing its compiled support or above site",
                node.index()
            ),
            Self::IncompleteWireNeighborhood { node, position } => {
                write!(f, "wire node {} is missing compiled query site at {position:?}", node.index())
            }
            Self::TickOutsideScheduler { delay } => write!(f, "imported tick delay {delay} exceeds the graph scheduler ring"),
            Self::InvalidZeroDelayPriority { priority } => {
                write!(f, "zero-delay graph tick has invalid priority {priority:?}")
            }
            Self::UnsupportedTickTarget { position, block_type } => {
                write!(f, "graph cannot dispatch tick for uncompiled block type {block_type} at {position:?}")
            }
            Self::UnknownTickTarget { position } => write!(
                f,
                "graph cannot classify imported tick target at {position:?} because its chunk was unavailable"
            ),
            Self::UnsupportedComponent { position, kind } => {
                write!(f, "graph execution does not support {kind} at {position:?}")
            }
        }
    }
}

impl std::error::Error for GraphBackendError {}

#[cfg(test)]
fn map_preparation_error(error: PreparationError) -> GraphBackendError {
    match error {
        PreparationError::Analysis(error) => GraphBackendError::Analysis(error),
        PreparationError::Geometry(error) => GraphBackendError::Geometry(error),
        PreparationError::Electrical(error) => GraphBackendError::Electrical(error),
        PreparationError::Callbacks(error) => GraphBackendError::Callbacks(error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ScheduledNodeTick {
    node: Option<NodeId>,
    position: BlockPos,
    block_type: Option<u32>,
    registry_type: u32,
    current_registry_type: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PendingNodeTick {
    /// This identifies a compiled receiver for pending-membership checks.
    node: Option<NodeId>,
    /// This identifies the scheduler position shared by equivalent tick entries.
    position: BlockPos,
    /// This preserves whether the imported tick carried an expected block type.
    block_type: Option<u32>,
    /// This identifies the effective scheduled type after resolving an absent type.
    registry_type: u32,
    /// This identifies the entry target type used for stale-tick checks.
    current_registry_type: u32,
}

impl From<ScheduledNodeTick> for PendingNodeTick {
    fn from(tick: ScheduledNodeTick) -> Self {
        Self {
            node: tick.node,
            position: tick.position,
            block_type: tick.block_type,
            registry_type: tick.registry_type,
            current_registry_type: tick.current_registry_type,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphPhase {
    BetweenTicks,
    ScheduledTicks,
    PistonActions,
    DeferredConnections,
}

#[derive(Debug, Clone, Copy)]
struct Clock {
    logical_tick: u64,
    phase: GraphPhase,
    scheduled_advanced: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum GraphObservedOperation {
    Callback { target: BlockPos, direction: Option<BlockFace> },
    Sample { actor: BlockPos, extended: bool, powered: bool },
    Request { actor: BlockPos, action: mchprs_world::PistonAction },
    AcceptanceAttempt { actor: BlockPos, action: mchprs_world::PistonAction },
    Accepted { actor: BlockPos, action: mchprs_world::PistonAction },
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestRecorder {
    // Recording and benchmark controls.
    /// Benchmark control; production reads the dense canonical pending index.
    general_pending_queries: bool,
    test_recording: bool,
    callback_census_enabled: bool,
    comparator_tick_focus: Option<(NodeId, u64)>,
    // Episode observations; clear() resets only these logs.
    trace: Vec<String>,
    callback_order: Vec<(BlockPos, DeliveryKind)>,
    comparator_updates: Vec<BlockPos>,
    observed_operations: Vec<GraphObservedOperation>,
    /// Native-format sample/applied trace at the same live request and acceptance cuts.
    native_piston_trace: Vec<crate::redstone::piston::trace::Entry>,
    /// Executed typed dispatch paths for logical BUD transition occurrences.
    logical_bud_direct_samples: FxHashMap<PistonId, u64>,
    logical_bud_boundary_callbacks: FxHashMap<PistonId, u64>,
    /// Owner-route rows skipped because the active owner is already busy.
    logical_bud_busy_callbacks_elided: u64,
    disable_busy_callback_contraction: bool,
    /// Owner event occurrences handed to the shared delivery path.
    logical_bud_occurrence_dispatches: u64,
    /// Cold reduced Main/Quasi evaluator sampled before accepted actor mutation.
    accepted_boundary_probe: Option<accepted_boundary_probe::Probe>,
    /// Test-only one-shot evaluator sampled at selected native actor/reset cuts.
    logical_firing_probe: Option<firing_probe::Probe>,
    /// Optional callback attribution; updated only by test builds and never changes trace order.
    callback_census: CallbackCensus,
    /// Opt-in per-value-region execution attribution for ignored research runs only.
    value_region_census_enabled: bool,
    value_region_census: ValueRegionExecutionCensus,
    // Experimental executor bindings and counters outlive individual trace episodes.
    power_links_examined: Cell<usize>,
    unchecked_classic: Option<Arc<unchecked_classic_anpu::UncheckedProgram>>,
    wire_tapes: Option<Arc<unchecked_classic_anpu::TapeProgram>>,
    fixed_classic_counts: fixed_classic::FixedClassicCounts,
    /// Optional per-wire seed calls/changes and frontier visits/changes for the active-net census.
    fixed_classic_activity: Vec<[u64; 4]>,
    /// Optional per-piston samples/actions for measuring prospective memory-cell ownership.
    memory_cell_activity: Vec<[u64; 2]>,
    /// Optional per-actor Instant attempt outcomes for the active CPU census.
    instant_kernel_activity: Vec<[u64; 7]>,
    piston_sample_census: piston_sample_census::EpisodeSampleCensus,
    guard_kind_census_enabled: bool,
    guard_kind_census: [Cell<u64>; 14],
}

#[cfg(test)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CallbackCensus {
    /// Accepted callbacks by [Update/Shape][Wire/Piston/Observer/ElectricalOther/Contact].
    pub(crate) accepted: [[u64; 5]; 2],
    /// Explicit piston receiver deliveries seen before any guard or no-op checks.
    pub(crate) piston_candidates: u64,
    /// Such candidates whose piston was already base-pending before guard evaluation.
    pub(crate) piston_candidates_base_pending: u64,
    /// Accepted piston deliveries whose receiver is base-pending at dispatch time.
    pub(crate) accepted_piston_base_pending: u64,
    pub(crate) transport_by_net: Vec<TransportNetCensus>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TransportNetCensus {
    pub(crate) invocations: u64,
    pub(crate) wire_visits: u64,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ValueRegionExecutionRow {
    pub(crate) dense_visits: u64,
    pub(crate) inert_visits: u64,
    pub(crate) changed_wire_writes: u64,
}

#[cfg(test)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ValueRegionExecutionCensus {
    /// Rows align with transport net, then the net's value-region table.
    pub(crate) by_net: Vec<Vec<ValueRegionExecutionRow>>,
    /// Complete transport invocations whose every wire visit was inert.
    pub(crate) all_inert_invocations: u64,
    pub(crate) all_inert_visits: u64,
}

#[cfg(test)]
fn callback_kind_index(kind: DeliveryKind) -> usize {
    match kind {
        DeliveryKind::Update { .. } => 0,
        DeliveryKind::Shape { .. } => 1,
    }
}

#[cfg(test)]
fn callback_family_index(receiver: CallbackReceiver, nodes: &[NodeSpec]) -> usize {
    match receiver {
        CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) => 1,
        CallbackReceiver::Contact(_) => 4,
        CallbackReceiver::Component(node) => match &nodes[node.index()].kind {
            CircuitNodeKind::Wire(_) => 0,
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::Electrical(NodeType::Observer { .. }) => 2,
            CircuitNodeKind::Electrical(_) => 3,
            _ => 3,
        },
    }
}

#[cfg(test)]
impl TestRecorder {
    fn clear(&mut self) {
        self.trace.clear();
        self.callback_order.clear();
        self.comparator_updates.clear();
        self.observed_operations.clear();
        self.native_piston_trace.clear();
        self.logical_bud_direct_samples.clear();
        self.logical_bud_boundary_callbacks.clear();
        self.callback_census = CallbackCensus::default();
        for rows in &mut self.value_region_census.by_net {
            rows.fill(ValueRegionExecutionRow::default());
        }
    }
}

enum PresentationEffect {
    BlockAction {
        position: BlockPos,
        action: BlockAction,
    },
    Sound {
        position: BlockPos,
        id: i32,
        category: i32,
        volume: f32,
        pitch: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A runtime failure that requires falling back from graph execution.
pub enum GraphExecutionError {
    /// The selected component kind has no graph execution implementation.
    UnsupportedComponent { position: Option<BlockPos>, kind: &'static str },
    /// Scheduled command block work reached execution without a world command host.
    CommandHostUnavailable { position: BlockPos },
}

impl fmt::Display for GraphExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedComponent { position, kind } => {
                write!(f, "graph execution does not support {kind} at {position:?}")
            }
            Self::CommandHostUnavailable { position } => {
                write!(f, "graph command block at {position:?} needs a host before its scheduled work can run")
            }
        }
    }
}

impl std::error::Error for GraphExecutionError {}

trait GraphCommandHost {
    fn publish_logical_tick(&mut self, tick: u64);
    fn run_command(&mut self, command: &str, source: &str) -> Result<(), String>;
    fn publish_command_entity(&mut self, position: BlockPos, entity: BlockEntity);
}

impl<W: World> GraphCommandHost for W {
    fn publish_logical_tick(&mut self, tick: u64) {
        self.piston_state_mut().logical_tick = tick;
    }

    fn run_command(&mut self, command: &str, source: &str) -> Result<(), String> {
        World::execute_command_block(self, command, source)
    }

    fn publish_command_entity(&mut self, position: BlockPos, entity: BlockEntity) {
        World::set_block_entity(self, position, entity);
    }
}

/// The running circuit: program, state, clock, queues, and display bookkeeping.
pub(crate) struct GraphBackend {
    program: CircuitProgram,
    state: CircuitState,
    positions: FxHashMap<BlockPos, NodeId>,
    component_sites: Vec<Option<SiteId>>,
    /// This pool reuses compiled hop scratch; each nested invocation owns a separate walk.
    wire_walks: Vec<WireWalk>,
    /// Closed transport programs reuse private scratch between propagation episodes.
    transport_scratch: Vec<transport::TransportScratch>,
    value_region_cache: Vec<transport::ValueRegionCache>,
    transition_cache: transitions::TransitionCache,
    /// False requires per-net checks; a noncanonical import or write permanently clears this hint.
    canonical_transport_repeaters: bool,
    stable_logic_scratch: Vec<stable_instant_logic::StableLogicScratch>,
    stable_logic_cache: Vec<stable_instant_logic::StableLogicCache>,
    classic_proof: Option<classic::ProofRecorder>,
    fixed_classic: Option<std::sync::Arc<FixedClassicProgram>>,
    fixed_classic_scratch: Vec<fixed_classic::FixedClassicScratch>,
    /// This holds reusable wave scratch and the current synchronous route replacement.
    instant_kernel: instant_kernel::KernelExecution,
    logical_firing: Option<logical_firing::LogicalFiringRuntime>,
    logical_devices: Vec<logical_device::LogicalDevice>,
    logical_device_owner: Vec<Option<usize>>,
    logical_device_admission: crate::redpiler::LogicalDeviceAdmission,
    active_frame_command_outputs: Vec<NodeId>,
    active_frame_retained_actors: Vec<PistonId>,
    active_frame_retained_nodes: Vec<NodeId>,
    active_frame_retained_routes: Vec<RouteOccurrence>,
    active_frame_power_inputs: Vec<logical_device::ActiveFramePowerInput>,
    /// O6 logical BUD owners index immutable transition plans by physical piston ID.
    /// Their values, revisions and deferred deadlines remain in the shared CircuitState/GraphFuture.
    logical_bud_owner_by_actor: Vec<Option<usize>>,
    /// Original callback occurrences that sample active logical BUD owners.
    logical_bud_samples_by_route: Vec<Option<Box<[Option<PistonId>]>>>,
    future: GraphFuture,
    dirty: Vec<bool>,
    /// Contact cells whose selected profile may differ from the last full projection.
    contact_dirty: Vec<bool>,
    /// Deduplicated queue so a no-op flush does not scan every compiled contact.
    dirty_contacts: Vec<ContactId>,
    contacts_by_piston: Vec<Vec<ContactId>>,
    contacts_by_resource: Vec<Vec<ContactId>>,
    always_dirty_contacts: Vec<ContactId>,
    full_contact_projection: bool,
    projected_pistons: Vec<PistonState>,
    projected_resources: Vec<ResourceState>,
    effects: Vec<PresentationEffect>,
    failure: Option<GraphExecutionError>,
    counts: GraphExecutionCounts,
    #[cfg(test)]
    test_recorder: TestRecorder,
}

/// All pending execution and its clock. Component/piston/resource values stay in CircuitState.
struct GraphFuture {
    scheduler: TickScheduler<ScheduledNodeTick>,
    pending: FxHashMap<PendingNodeTick, usize>,
    /// Exact multiplicity of canonical typed ticks, indexed by immutable component ID.
    pending_nodes: Box<[usize]>,
    piston_requests: VecDeque<PistonRequest>,
    deferred_pistons: VecDeque<DeferredPistonEvent>,
    clock: Clock,
}

#[cfg(test)]
impl Deref for GraphBackend {
    type Target = TestRecorder;

    fn deref(&self) -> &Self::Target {
        &self.test_recorder
    }
}

#[cfg(test)]
impl DerefMut for GraphBackend {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.test_recorder
    }
}

impl GraphBackend {
    #[cfg(test)]
    pub(super) fn record_callback_candidate(&mut self, receiver: CallbackReceiver) {
        if !self.test_recorder.callback_census_enabled {
            return;
        }
        if let CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) = receiver {
            self.test_recorder.callback_census.piston_candidates += 1;
            self.test_recorder.callback_census.piston_candidates_base_pending +=
                u64::from(self.state.pistons[piston.index()].availability.base_pending());
        }
    }

    #[cfg(test)]
    pub(super) fn record_accepted_callback(&mut self, kind: DeliveryKind, receiver: CallbackReceiver) {
        if !self.test_recorder.callback_census_enabled {
            return;
        }
        let census = &mut self.test_recorder.callback_census;
        let event_kind = callback_kind_index(kind);
        let family = callback_family_index(receiver, self.program.nodes());
        census.accepted[event_kind][family] += 1;
        if let CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) = receiver {
            census.accepted_piston_base_pending += u64::from(self.state.pistons[piston.index()].availability.base_pending());
        }
    }

    /// Create a bare Graph executor without production activation.
    ///
    /// The returned executor remains usable for ordinary Graph execution. Use `from_lowered`
    /// when activation is required: it imports ticks, installs the logical firing/device
    /// runtimes, and schedules initial command ticks.
    pub(crate) fn new_unactivated(program: CircuitProgram, state: CircuitState, logical_tick: u64) -> Result<Self, GraphBackendError> {
        let positions = program.position_index();
        Self::new_with_positions(program, state, logical_tick, positions)
    }

    #[cfg(test)]
    pub(crate) fn compile<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
    ) -> Result<Self, GraphBackendError> {
        Self::compile_with_options(world, bounds, ticks, &crate::redpiler::CompilerOptions::default(), monitor)
    }

    #[cfg(test)]
    pub(crate) fn compile_with_options<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        options: &crate::redpiler::CompilerOptions,
        monitor: Arc<TaskMonitor>,
    ) -> Result<Self, GraphBackendError> {
        let pipeline = passes::default_graph_pipeline(options);
        Self::compile_with_pipeline(world, bounds, ticks, options, &pipeline, monitor)
    }

    #[cfg(test)]
    pub(crate) fn compile_without_fixed_classic<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        options: &crate::redpiler::CompilerOptions,
        monitor: Arc<TaskMonitor>,
    ) -> Result<Self, GraphBackendError> {
        // Keep research controls unowned when O2 enables both classic compilers.
        let mut pipeline = passes::default_graph_pipeline(options);
        pipeline
            .entries
            .retain(|entry| !matches!(&entry.pass, passes::Pass::Build(builder) if matches!(builder.name(), "compile-fixed-classic" | "compile-classic-redstone")));
        Self::compile_with_pipeline(world, bounds, ticks, options, &pipeline, monitor)
    }

    #[cfg(test)]
    pub(crate) fn compile_with_pipeline<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        options: &crate::redpiler::CompilerOptions,
        pipeline: &passes::PassPipeline,
        monitor: Arc<TaskMonitor>,
    ) -> Result<Self, GraphBackendError> {
        let limits = AnalysisLimits::for_budget(monitor.budget_multiplier());
        let prepared = passes::rich::prepare(world, bounds, ticks, &monitor, limits).map_err(map_preparation_error)?;
        let prepared = prepared
            .run_passes(options, pipeline, &monitor)
            .map_err(GraphBackendError::PassPipeline)?;
        let lowered = prepared.lower(&monitor).map_err(GraphBackendError::Graph)?;
        Self::from_lowered(lowered, &monitor)
    }

    /// Create an executor from the program, starting state, clock, and pending ticks.
    /// Checks runtime support and schedules initial command ticks. The caller
    /// makes the backend active after this succeeds.
    pub(crate) fn from_lowered(lowered: LoweredCircuit, monitor: &TaskMonitor) -> Result<Self, GraphBackendError> {
        Self::validate_supported(&lowered.program)?;
        let mut backend = Self::new_with_positions(lowered.program, lowered.state, lowered.logical_tick, lowered.positions)?;
        backend.import_ticks(lowered.imported_ticks)?;
        backend.logical_firing = logical_firing::LogicalFiringRuntime::new(&backend);
        backend.logical_device_admission = backend.install_logical_device().map_err(|_| GraphBackendError::UnsupportedComponent {
            position: None,
            kind: "logical device input or stationary entry contract",
        })?;
        let initial_command_ticks: Vec<_> = backend
            .program
            .command_blocks()
            .iter()
            .filter_map(|plan| {
                if matches!(
                    backend.program.nodes()[plan.node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::CommandBlock {
                        initial_tick: true,
                        chain: false,
                        ..
                    })
                ) {
                    Some(plan.node)
                } else {
                    None
                }
            })
            .collect();
        for node in initial_command_ticks {
            if backend.is_pending(node) {
                continue;
            }
            let condition_met = backend.command_condition_met(node);
            if let ComponentState::Electrical(state) = &mut backend.state.components[node.index()]
                && let Some(entity) = &mut state.command_entity
            {
                entity.condition_met = condition_met;
                backend.dirty[node.index()] = true;
            }
            backend.schedule_half_tick(node, 1, TickPriority::Normal);
        }
        if monitor.cancelled() {
            return Err(GraphBackendError::Cancelled);
        }
        Ok(backend)
    }

    fn validate_supported(program: &CircuitProgram) -> Result<(), GraphBackendError> {
        for node in program.nodes() {
            let unsupported = match node.kind {
                CircuitNodeKind::Electrical(NodeType::Trapdoor) => Some("trapdoor"),
                _ => None,
            };
            if let Some(kind) = unsupported {
                return Err(GraphBackendError::UnsupportedComponent {
                    position: node.position,
                    kind,
                });
            }
        }
        for node in program.nodes() {
            let CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) = node.kind else {
                continue;
            };
            let Some(position) = node.position else {
                continue;
            };
            let Some(block) = node.block else { continue };
            let facing = command_facing(block);
            for (neighbor, only_chain, required) in [
                (position.offset(facing), true, true),
                (position.offset(facing.opposite()), false, block.property("conditional") == Some("true")),
            ] {
                if !required {
                    continue;
                }
                let uncompiled_command = program
                    .sites()
                    .iter()
                    .find(|site| site.position == neighbor)
                    .and_then(|site| match site.source {
                        SiteSource::Constant(profile) => program.profile_block(profile),
                        SiteSource::Component(component) => program.nodes()[component.index()].block,
                        SiteSource::Contact(contact) => program.contacts()[contact.index()]
                            .variants
                            .iter()
                            .filter_map(|variant| program.profile_block(variant.profile))
                            .find(|block| block.is_command_block()),
                    })
                    .is_some_and(|neighbor_block| {
                        neighbor_block.is_command_block()
                            && (!only_chain || neighbor_block.get_name() == "chain_command_block")
                            && !program
                                .command_blocks()
                                .iter()
                                .any(|plan| program.nodes()[plan.node.index()].position == Some(neighbor))
                    });
                if uncompiled_command {
                    return Err(GraphBackendError::UnsupportedComponent {
                        position: Some(neighbor),
                        kind: "command chain or conditional predecessor outside the selection",
                    });
                }
            }
        }
        for (index, node) in program.nodes().iter().enumerate() {
            if matches!(node.kind, CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }))
                && !program.note_blocks().iter().any(|plan| plan.node.index() == index)
            {
                return Err(GraphBackendError::MissingNoteBlockSite {
                    node: NodeId::new(index as u32),
                    position: node.position.expect("noteblocks have positions"),
                });
            }
        }
        let external_wires: FxHashSet<_> = program
            .sites()
            .iter()
            .enumerate()
            .filter_map(|(index, site)| {
                let SiteSource::Constant(profile) = site.source else {
                    return None;
                };
                matches!(program.profile_block(profile), Some(Block::RedstoneWire { .. })).then_some(SiteId::new(index as u32))
            })
            .collect();
        let mut conducted_users: FxHashMap<PowerTarget, Vec<PowerTarget>> = FxHashMap::default();
        let mut pending = Vec::new();
        for link in program.power_links() {
            if let PowerPort::Conducted { include_dust } = link.from_port {
                conducted_users
                    .entry(PowerTarget::Conductor {
                        site: link.from,
                        include_dust,
                    })
                    .or_default()
                    .push(link.to);
            }
            if external_wires.contains(&link.from) && matches!(link.from_port, PowerPort::Weak(_) | PowerPort::Strong(_) | PowerPort::Stored) {
                pending.push((link.to, link.from));
            }
        }
        let mut visited = FxHashSet::default();
        while let Some((target, wire)) = pending.pop() {
            if !visited.insert(target) {
                continue;
            }
            match target {
                PowerTarget::Input { .. } => {
                    return Err(GraphBackendError::UnsupportedComponent {
                        position: Some(program.sites()[wire.index()].position),
                        kind: "external redstone wire boundary",
                    });
                }
                PowerTarget::Conductor { .. } => {
                    pending.extend(conducted_users.get(&target).into_iter().flatten().map(|user| (*user, wire)));
                }
            }
        }
        Ok(())
    }

    fn new_with_positions(
        program: CircuitProgram,
        mut state: CircuitState,
        logical_tick: u64,
        positions: FxHashMap<BlockPos, NodeId>,
    ) -> Result<Self, GraphBackendError> {
        if let Some(fixed) = program.fixed_classic() {
            for wire in &fixed.wires {
                if let Some(expected) = wire.inert_sides
                    && !matches!(state.components.get(wire.node.index()), Some(ComponentState::Wire(value))
                        if FixedClassicWire::packed_sides(*value) == expected)
                {
                    return Err(GraphBackendError::UnsupportedComponent {
                        position: program.nodes()[wire.node.index()].position,
                        kind: "fixed classic wire sides differ from certified entry",
                    });
                }
            }
        }
        let mut component_sites = vec![None; program.nodes().len()];
        for (index, node) in program.nodes().iter().enumerate() {
            let id = NodeId::try_from_index(index).expect("graph node count was validated");
            if matches!(
                node.kind,
                CircuitNodeKind::Electrical(_) | CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } | CircuitNodeKind::Wire(_)
            ) {
                if node.raw_state.is_none() {
                    return Err(GraphBackendError::MissingBlockState { node: id });
                }
            }
        }
        for (index, site) in program.sites().iter().enumerate() {
            if let SiteSource::Component(node) = site.source {
                component_sites[node.index()] = Some(SiteId::new(index as u32));
            }
        }
        for wire in program.wires() {
            let Some(root_site) = component_sites[wire.node.index()] else {
                return Err(GraphBackendError::MissingComponentSite { node: wire.node });
            };
            let position = program.nodes()[wire.node.index()].position.expect("wire nodes have positions");
            if program.sites()[root_site.index()].position != position {
                return Err(GraphBackendError::MissingComponentSite { node: wire.node });
            }
            for (site, expected) in wire.canonical_neighbors.iter().zip(crate::world::wire_cache::positions(position)) {
                if program.sites()[site.index()].position != expected {
                    return Err(GraphBackendError::IncompleteWireNeighborhood {
                        node: wire.node,
                        position: expected,
                    });
                }
            }
        }
        // Imported component flags are descriptive only; scheduler membership owns pending work.
        for component in &mut state.components {
            if let ComponentState::Electrical(electrical) = component {
                electrical.pending_tick = false;
            }
        }
        let node_count = program.nodes().len();
        let (contacts_by_piston, contacts_by_resource, always_dirty_contacts) = projection::contact_projection_dependencies(&program);
        let contact_count = program.contacts().len();
        let full_contact_projection = program.contacts().iter().map(|contact| contact.position).collect::<FxHashSet<_>>().len() != contact_count;
        let contact_dirty = vec![true; contact_count];
        let dirty_contacts = (0..contact_count).map(|index| ContactId::new(index as u32)).collect();
        let projected_pistons = state.pistons.to_vec();
        let projected_resources = state.resources.to_vec();
        let fixed_classic = program.fixed_classic().cloned();
        let instant_kernel = instant_kernel::KernelExecution::new(&program);
        let mut logical_bud_owner_by_actor = vec![None; program.pistons().len()];
        let mut logical_bud_samples_by_route: Vec<Option<Box<[Option<PistonId>]>>> =
            std::iter::repeat_with(|| None).take(program.routes().len()).collect();
        let instant_kernel_routes: FxHashSet<_> = program
            .instant_kernels()
            .iter()
            .flat_map(|kernel| kernel.routes.iter().map(|route| route.route))
            .collect();
        for (index, owner) in program.logical_bud_owners().iter().enumerate() {
            let piston_state = state.pistons.get(owner.actor.index());
            let resource_state = state.resources.get(owner.resource.index());
            let matches_entry = piston_state.is_some_and(|current| {
                current.extended == owner.initial_piston.extended
                    && current.head_present == owner.initial_piston.head_present
                    && current.availability == owner.initial_piston.availability
            }) && resource_state
                .is_some_and(|current| current.at == owner.initial_resource.at && current.enabled == owner.initial_resource.enabled);
            // The older pull-wave route replacement cannot preserve an individual occurrence
            // handoff to a logical owner. Keep that actor physical when the route programs overlap.
            let route_conflict = owner
                .sample_routes
                .iter()
                .any(|occurrence| instant_kernel_routes.contains(&occurrence.route) || program.instant_route_binding(occurrence.route).is_some());
            if matches_entry && !route_conflict {
                logical_bud_owner_by_actor[owner.actor.index()] = Some(index);
                for &occurrence in owner.sample_routes.iter() {
                    let samples = &mut logical_bud_samples_by_route[occurrence.route.index()];
                    let ordinals =
                        samples.get_or_insert_with(|| vec![None; program.routes()[occurrence.route.index()].deliveries.len()].into_boxed_slice());
                    if let Some(actor) = ordinals.get_mut(occurrence.ordinal as usize) {
                        *actor = Some(owner.actor);
                    }
                }
            }
        }
        let canonical_transport_repeaters = program.nodes().iter().zip(state.components.iter()).all(|(node, state)| {
            !matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. }))
                || matches!(state, ComponentState::Electrical(value) if value.output_strength == 15 * u8::from(value.powered))
        });
        Ok(Self {
            program,
            state,
            positions,
            component_sites,
            wire_walks: Vec::new(),
            transport_scratch: Vec::new(),
            value_region_cache: Vec::new(),
            transition_cache: transitions::TransitionCache::default(),
            canonical_transport_repeaters,
            stable_logic_scratch: Vec::new(),
            stable_logic_cache: Vec::new(),
            classic_proof: None,
            fixed_classic,
            fixed_classic_scratch: Vec::new(),
            instant_kernel,
            logical_firing: None,
            logical_devices: Vec::new(),
            logical_device_owner: Vec::new(),
            logical_device_admission: crate::redpiler::LogicalDeviceAdmission::NotApplicable,
            active_frame_command_outputs: Vec::new(),
            active_frame_retained_actors: Vec::new(),
            active_frame_retained_nodes: Vec::new(),
            active_frame_retained_routes: Vec::new(),
            active_frame_power_inputs: Vec::new(),
            logical_bud_owner_by_actor,
            logical_bud_samples_by_route,
            future: GraphFuture {
                scheduler: TickScheduler::default(),
                pending: FxHashMap::default(),
                pending_nodes: vec![0; node_count].into_boxed_slice(),
                piston_requests: VecDeque::new(),
                deferred_pistons: VecDeque::new(),
                clock: Clock {
                    logical_tick,
                    phase: GraphPhase::BetweenTicks,
                    scheduled_advanced: false,
                },
            },
            dirty: vec![false; node_count],
            contact_dirty,
            dirty_contacts,
            contacts_by_piston,
            contacts_by_resource,
            always_dirty_contacts,
            full_contact_projection,
            projected_pistons,
            projected_resources,
            effects: Vec::new(),
            failure: None,
            counts: GraphExecutionCounts::default(),
            #[cfg(test)]
            test_recorder: TestRecorder::default(),
        })
    }

    fn import_ticks(&mut self, ticks: Vec<ImportedTick>) -> Result<(), GraphBackendError> {
        for tick in ticks {
            if tick.delay as usize >= SCHEDULER_BUCKETS {
                return Err(GraphBackendError::TickOutsideScheduler { delay: tick.delay });
            }
            if tick.delay == 0 && tick.priority != TickPriority::NanoTick {
                return Err(GraphBackendError::InvalidZeroDelayPriority { priority: tick.priority });
            }
            let current_registry_type = tick
                .current_registry_type
                .ok_or(GraphBackendError::UnknownTickTarget { position: tick.position })?;
            let node = tick.target;
            if let Some(node) = node {
                let Some(spec) = self.program.nodes().get(node.index()) else {
                    return Err(GraphBackendError::UnsupportedTickTarget {
                        position: tick.position,
                        block_type: current_registry_type,
                    });
                };
                if spec.position != Some(tick.position)
                    || self.positions.get(&tick.position) != Some(&node)
                    || self.registry_type(node) != current_registry_type
                {
                    return Err(GraphBackendError::UnsupportedTickTarget {
                        position: tick.position,
                        block_type: current_registry_type,
                    });
                }
            }
            let registry_type = tick.block_type.unwrap_or(current_registry_type);
            if node.is_none() && registry_type == current_registry_type && current_registry_type != Block::Air.registry_id() {
                return Err(GraphBackendError::UnsupportedTickTarget {
                    position: tick.position,
                    block_type: current_registry_type,
                });
            }
            let scheduled = ScheduledNodeTick {
                node,
                position: tick.position,
                block_type: tick.block_type,
                registry_type,
                current_registry_type,
            };
            self.future.scheduler.schedule_half_tick(scheduled, tick.delay as usize, tick.priority);
            *self.future.pending.entry(scheduled.into()).or_default() += 1;
            if let Some(node) = self.canonical_pending_node(scheduled) {
                self.future.pending_nodes[node.index()] += 1;
            }
        }
        Ok(())
    }

    fn registry_type(&self, node: NodeId) -> u32 {
        self.program.nodes()[node.index()].block.unwrap().registry_id()
    }

    fn schedule(&mut self, node: NodeId, delay: usize, priority: TickPriority) {
        self.schedule_with(node, delay.saturating_mul(2), priority, true);
    }

    fn schedule_half_tick(&mut self, node: NodeId, delay: usize, priority: TickPriority) {
        self.schedule_with(node, delay, priority, false);
    }

    fn schedule_with(&mut self, node: NodeId, delay: usize, priority: TickPriority, _game_tick_delay: bool) {
        if delay >= SCHEDULER_BUCKETS || (delay == 0 && priority != TickPriority::NanoTick) {
            return;
        }
        if self.is_pending(node) {
            return;
        }
        let registry_type = self.registry_type(node);
        let tick = ScheduledNodeTick {
            node: Some(node),
            position: self.position(node),
            block_type: Some(registry_type),
            registry_type,
            current_registry_type: registry_type,
        };
        let key = tick.into();
        self.future.scheduler.schedule_half_tick(tick, delay, priority);
        self.future.pending.insert(key, 1);
        self.future.pending_nodes[node.index()] = 1;
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "schedule {:?} +{} {} {priority:?}",
                node,
                if _game_tick_delay { delay / 2 } else { delay },
                if _game_tick_delay { "ticks" } else { "half-ticks" }
            )
        );
    }

    fn is_pending(&self, node: NodeId) -> bool {
        #[cfg(test)]
        if self.test_recorder.general_pending_queries {
            return self.pending_from_general_map(node);
        }
        self.future.pending_nodes[node.index()] != 0
    }

    /// Untyped, stale and unbound imported events are distinct pending identities.
    fn canonical_pending_node(&self, tick: ScheduledNodeTick) -> Option<NodeId> {
        let node = tick.node?;
        let spec = self.program.nodes().get(node.index())?;
        let registry_type = spec.block?.registry_id();
        (spec.position == Some(tick.position)
            && tick.block_type == Some(registry_type)
            && tick.registry_type == registry_type
            && tick.current_registry_type == registry_type)
            .then_some(node)
    }

    #[cfg(test)]
    fn pending_from_general_map(&self, node: NodeId) -> bool {
        self.future.pending.contains_key(&PendingNodeTick {
            node: Some(node),
            position: self.position(node),
            block_type: Some(self.registry_type(node)),
            registry_type: self.registry_type(node),
            current_registry_type: self.registry_type(node),
        })
    }

    fn popped(&mut self, tick: ScheduledNodeTick) {
        let key = tick.into();
        let canonical = self.canonical_pending_node(tick);
        if let Some(count) = self.future.pending.get_mut(&key) {
            *count -= 1;
            if let Some(node) = canonical {
                debug_assert!(self.future.pending_nodes[node.index()] > 0);
                self.future.pending_nodes[node.index()] -= 1;
            }
            if *count == 0 {
                self.future.pending.remove(&key);
            }
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), GraphExecutionError> {
        self.tick_with_host(None)
    }

    #[cfg(test)]
    pub(crate) fn circuit_state(&self) -> &CircuitState {
        &self.state
    }

    #[cfg(test)]
    pub(crate) fn assert_same_execution_state(&self, other: &Self) {
        assert_eq!(self.state, other.state, "authoritative graph state");
        assert_eq!(self.future.clock.logical_tick, other.future.clock.logical_tick);
        assert_eq!(self.future.clock.phase, other.future.clock.phase);
        assert_eq!(self.future.clock.scheduled_advanced, other.future.clock.scheduled_advanced);
        assert_eq!(
            self.future.scheduler.iter().collect::<Vec<_>>(),
            other.future.scheduler.iter().collect::<Vec<_>>()
        );
        assert_eq!(self.future.pending, other.future.pending);
        assert_eq!(self.future.pending_nodes, other.future.pending_nodes);
        assert_eq!(self.future.piston_requests, other.future.piston_requests);
        assert_eq!(self.future.deferred_pistons, other.future.deferred_pistons);
    }

    pub(crate) fn tick_with_world<W: World>(&mut self, world: &mut W) -> Result<(), GraphExecutionError> {
        self.tick_with_host(Some(world))
    }

    /// Run a tick, checking for a command host before advancing the clock.
    /// A missing host can be supplied on retry. Other errors stop later execution.
    fn tick_with_host<'h>(&mut self, mut host: Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if host.is_none()
            && let Some(position) = self.command_host_preflight_position()
        {
            return Err(GraphExecutionError::CommandHostUnavailable { position });
        }
        if !self.logical_devices.is_empty() {
            let result = self.tick_logical_device(&mut host);
            self.latch_error(&result);
            return result;
        }
        let result = self.tick_phases(&mut host);
        if let Err(error) = &result {
            if !matches!(error, GraphExecutionError::CommandHostUnavailable { .. }) {
                self.failure = Some(error.clone());
            }
        }
        result
    }

    fn due_command_position(&self) -> Option<BlockPos> {
        self.future
            .scheduler
            .iter()
            .find(|(tick, delay, _)| {
                *delay <= 1
                    && tick.registry_type == tick.current_registry_type
                    && tick.node.is_some_and(|node| {
                        matches!(
                            self.program.nodes()[node.index()].kind,
                            CircuitNodeKind::Electrical(NodeType::CommandBlock { chain: false, .. })
                        )
                    })
            })
            .map(|(tick, _, _)| tick.position)
    }

    fn command_host_preflight_position(&self) -> Option<BlockPos> {
        self.due_command_position().or_else(|| {
            if !self.future.scheduler.iter().any(|(_, delay, _)| delay <= 1) {
                return None;
            }
            // shortcut: any due work in a command-containing graph may activate a command, until callback reachability can prove it cannot.
            self.program
                .command_blocks()
                .iter()
                .find(|plan| {
                    !matches!(
                        self.program.nodes()[plan.node.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::CommandBlock { chain: true, .. })
                    )
                })
                .map(|plan| self.position(plan.node))
        })
    }

    /// Run due ticks, advance the scheduler and check again, then handle piston
    /// actions and deferred connections. Callbacks can add work during each phase.
    fn tick_phases<'h>(&mut self, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
        self.tick_phases_with_hook(host, |_, _| Ok(()))
    }

    /// Run the ordinary graph tick with one source-derived computation hook
    /// after scheduled callbacks and before piston sampling/actions.
    fn tick_phases_with_hook<'h>(
        &mut self,
        host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>,
        mut before_piston_actions: impl FnMut(&mut Self, &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError>,
    ) -> Result<(), GraphExecutionError> {
        self.counts.ticks += 1;
        self.future.clock.logical_tick += 1;
        if let Some(host) = host.as_mut() {
            host.publish_logical_tick(self.future.clock.logical_tick);
        }
        self.future.clock.phase = GraphPhase::ScheduledTicks;
        self.future.clock.scheduled_advanced = false;
        graph_trace!(self.future.clock.logical_tick, self.future.clock.phase, "phase start");
        #[cfg(test)]
        graph_test_trace!(self, "phase scheduled_ticks".into());
        let drain_result = self.drain_current_ticks(host);
        if let Err(error) = drain_result {
            self.future.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }

        self.future.scheduler.end_last_tick_move_next();
        self.future.clock.scheduled_advanced = true;
        #[cfg(test)]
        graph_test_trace!(self, "clock.advance".into());
        #[cfg(test)]
        graph_test_trace!(self, "phase scheduled_ticks_after_advance".into());
        let drain_result = self.drain_current_ticks(host);
        if let Err(error) = drain_result {
            self.future.clock.phase = GraphPhase::BetweenTicks;
            self.future.clock.scheduled_advanced = false;
            return Err(error);
        }

        if let Err(error) = before_piston_actions(self, host) {
            self.future.clock.phase = GraphPhase::BetweenTicks;
            self.future.clock.scheduled_advanced = false;
            return Err(error);
        }

        self.future.clock.phase = GraphPhase::PistonActions;
        graph_trace!(self.future.clock.logical_tick, self.future.clock.phase, "phase start");
        #[cfg(test)]
        graph_test_trace!(self, "phase piston_actions".into());
        let action_result = self.drain_piston_actions();
        if let Err(error) = action_result {
            self.future.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }
        self.future.clock.phase = GraphPhase::DeferredConnections;
        graph_trace!(self.future.clock.logical_tick, self.future.clock.phase, "phase start");
        #[cfg(test)]
        graph_test_trace!(self, "phase deferred_connections".into());
        let deferred_result = self.publish_deferred_pistons();
        if let Err(error) = deferred_result {
            self.future.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }
        self.future.clock.phase = GraphPhase::BetweenTicks;
        self.future.clock.scheduled_advanced = false;
        #[cfg(test)]
        graph_test_trace!(self, "phase between_ticks".into());
        Ok(())
    }

    fn drain_current_ticks<'h>(&mut self, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
        while let Some(tick) = self.future.scheduler.this_tick().pop_first() {
            self.popped(tick);
            if tick.registry_type != tick.current_registry_type {
                #[cfg(test)]
                graph_test_trace!(self, format!("stale {:?} skipped", tick.position));
                continue;
            }
            let Some(node) = tick.node else {
                #[cfg(test)]
                graph_test_trace!(self, format!("unbound {:?} skipped", tick.position));
                continue;
            };
            graph_trace!(self.future.clock.logical_tick, self.future.clock.phase, "dispatch node={}", node.index());
            #[cfg(test)]
            graph_test_trace!(self, format!("dispatch {node:?}"));
            self.counts.scheduled_ticks += 1;
            self.tick_component(node, host)?;
        }
        Ok(())
    }

    pub(crate) fn use_block(&mut self, position: BlockPos) -> Result<(), GraphExecutionError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.use_block_inner(position);
        self.latch_error(&result);
        result
    }

    fn use_block_inner(&mut self, position: BlockPos) -> Result<(), GraphExecutionError> {
        let Some(&node) = self.positions.get(&position) else {
            return Ok(());
        };
        match self.program.nodes()[node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::Lever) => {
                let powered = !self.node_state(node).powered;
                self.set_node(node, powered, u8::from(powered) * 15, None, !self.logical_node_owned(node))?;
            }
            CircuitNodeKind::Electrical(NodeType::Button) => {
                if !self.node_state(node).powered {
                    self.schedule(node, 10, TickPriority::Normal);
                    self.set_node(node, true, 15, None, !self.logical_node_owned(node))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn set_pressure_plate(&mut self, position: BlockPos, powered: bool) -> Result<(), GraphExecutionError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.set_pressure_plate_inner(position, powered);
        self.latch_error(&result);
        result
    }

    fn set_pressure_plate_inner(&mut self, position: BlockPos, powered: bool) -> Result<(), GraphExecutionError> {
        let Some(&node) = self.positions.get(&position) else {
            return Ok(());
        };
        if matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::PressurePlate)
        ) {
            self.set_node(node, powered, u8::from(powered) * 15, None, !self.logical_node_owned(node))?;
        }
        Ok(())
    }

    fn latch_error(&mut self, result: &Result<(), GraphExecutionError>) {
        if let Err(error) = result {
            self.failure = Some(error.clone());
        }
    }

    /// Run a component's scheduled tick and send any resulting notifications.
    fn tick_component<'h>(&mut self, node: NodeId, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
        if self.logical_node_owned(node) && !self.active_frame_retained_nodes.contains(&node) {
            return Err(self.unsupported(node, "logical frame received an unowned scheduled tick"));
        }
        if self.fixed_classic.is_some() {
            return self.fixed_classic_tick(node);
        }
        if self.logical_firing.as_ref().is_some_and(|runtime| runtime.owns_observer(node)) {
            return self.tick_logical_firing_observer(node);
        }
        match self.program.nodes()[node.index()].kind.clone() {
            CircuitNodeKind::Wire(_) => Ok(()),
            CircuitNodeKind::Observer { .. } => self.tick_observer(node),
            CircuitNodeKind::BuiltinReset { .. } => self.tick_builtin_reset(node),
            CircuitNodeKind::Electrical(NodeType::Torch) => self.tick_torch(node),
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, .. }) => self.tick_repeater(node, delay),
            CircuitNodeKind::Electrical(NodeType::Comparator { mode, facing_diode, .. }) => self.tick_comparator(node, mode, facing_diode),
            CircuitNodeKind::Electrical(NodeType::Lamp) => self.tick_lamp(node),
            CircuitNodeKind::Electrical(NodeType::Button) => {
                if self.node_state(node).powered {
                    self.effects.push(PresentationEffect::Sound {
                        position: self.position(node),
                        id: crate::sound::event_id("block.stone_button.click_off"),
                        category: 4,
                        volume: 1.0,
                        pitch: 1.0,
                    });
                    self.set_node(node, false, 0, None, true)?;
                }
                Ok(())
            }
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::PressurePlate) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::CopperBulb | NodeType::Hopper) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Constant) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Observer { .. } | NodeType::Wire) => {
                Err(self.unsupported(node, "invalid generic observer or wire"))
            }
            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => self.tick_command_block(node, host),
            CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Trapdoor) => Err(self.unsupported(node, "trapdoor")),
            CircuitNodeKind::Piston(piston) => self.sample_piston(piston),
            CircuitNodeKind::Contact(_) => Ok(()),
        }
    }

    /// Handle a notification using live or captured receiver state.
    /// The component may change immediately or schedule a tick.
    fn update_component(&mut self, node: NodeId, kind: DeliveryKind, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        if self.classic_proof.is_some()
            && matches!(kind, DeliveryKind::Update { .. })
            && !matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_))
        {
            return self.record_classic_sample(node, captured);
        }
        let direction = match kind {
            DeliveryKind::Update { direction } => direction,
            DeliveryKind::Shape { direction } => {
                return if matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)) {
                    self.update_wire_shape(node, direction, captured)
                } else {
                    Ok(())
                };
            }
        };
        #[cfg(test)]
        if self.test_recording {
            let position = self.position(node);
            self.test_recorder.callback_order.push((position, kind));
        }
        match self.program.nodes()[node.index()].kind.clone() {
            CircuitNodeKind::Observer { .. } => self.update_observer(node, direction, captured),
            CircuitNodeKind::BuiltinReset { facing, .. } => {
                self.arm_builtin_reset(node, facing, direction, captured);
                Ok(())
            }
            CircuitNodeKind::Electrical(NodeType::Torch) => self.update_torch(node, captured),
            CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => self.update_repeater(node, captured),
            CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => self.update_comparator(node),
            CircuitNodeKind::Electrical(NodeType::Lamp) => self.update_lamp(node, captured),
            CircuitNodeKind::Electrical(NodeType::CopperBulb) => self.update_bulb(node),
            CircuitNodeKind::Electrical(NodeType::Hopper) => self.update_hopper(node, captured),
            CircuitNodeKind::Wire(_) => self.evaluate_seed(node, captured),
            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => self.update_command_power(node),
            CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => self.update_note(node),
            CircuitNodeKind::Electrical(NodeType::Trapdoor) => Err(self.unsupported(node, "trapdoor")),
            CircuitNodeKind::Piston(piston) => self.sample_piston(piston),
            CircuitNodeKind::Contact(_) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Constant) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Observer { .. } | NodeType::Wire) => {
                Err(self.unsupported(node, "invalid generic observer or wire"))
            }
        }
    }

    fn command_plan(&self, node: NodeId) -> Option<CommandBlockPlan> {
        self.program.command_plan(node)
    }

    fn command_condition_met(&self, node: NodeId) -> bool {
        let Some(plan) = self.command_plan(node) else {
            return false;
        };
        if !plan.conditional {
            return true;
        }
        plan.previous.is_some_and(|previous| {
            self.node_state(previous)
                .command_entity
                .as_ref()
                .is_some_and(|entity| entity.success_count > 0)
        })
    }

    fn update_command_power(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        if self.active_frame_command_outputs.contains(&node) {
            return Ok(());
        }
        let powered = self.input_strength(node, InputPort::Main) > 0;
        self.update_command_powered(node, powered)
    }

    fn update_command_powered(&mut self, node: NodeId, powered: bool) -> Result<(), GraphExecutionError> {
        let (repeating, chain, automatic) = match self.program.nodes()[node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::CommandBlock {
                repeating, chain, automatic, ..
            }) => (repeating, chain, automatic),
            _ => return Err(self.unsupported(node, "invalid command block binding")),
        };
        let missing_entity = self.node_state(node).command_entity.is_none();
        if let ComponentState::Electrical(state) = &mut self.state.components[node.index()] {
            state.command_entity.get_or_insert_with(Default::default);
        }
        let entity = self.node_state(node).command_entity.as_ref().unwrap();
        let entity_powered = entity.powered;
        let last_execution = entity.last_execution;
        let rising = powered && !entity_powered;
        let changed = entity_powered != powered;
        let should_start = rising || (automatic && last_execution < 0);
        let should_repeat = repeating && (powered || automatic);
        let should_schedule = !chain && (should_start || should_repeat) && !self.is_pending(node);
        let condition_met = should_schedule.then(|| self.command_condition_met(node));
        let position = self.position(node);
        let Some(ComponentState::Electrical(state)) = self.state.components.get_mut(node.index()) else {
            return Err(GraphExecutionError::UnsupportedComponent {
                position: Some(position),
                kind: "command block state",
            });
        };
        let Some(entity) = state.command_entity.as_mut() else {
            return Err(GraphExecutionError::UnsupportedComponent {
                position: Some(position),
                kind: "command block entity state",
            });
        };
        state.powered = powered;
        entity.powered = powered;
        if let Some(condition_met) = condition_met {
            entity.condition_met = condition_met;
        }
        if missing_entity || should_schedule || changed {
            self.dirty[node.index()] = true;
        }
        if should_schedule {
            self.schedule_half_tick(node, 1, TickPriority::Normal);
        }
        Ok(())
    }

    fn tick_command_block<'h>(&mut self, node: NodeId, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
        if self.command_plan(node).is_none() {
            return Err(self.unsupported(node, "command block plan"));
        }
        if matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::CommandBlock { chain: true, .. })
        ) {
            return Ok(());
        }
        if !self.execute_command(node, host)? {
            return Ok(());
        }
        let mut current = node;
        let mut may_continue = self.node_state(node).command_entity.as_ref().is_some_and(|entity| entity.condition_met);
        for _ in 0..256 {
            if !may_continue {
                break;
            }
            let Some(next) = self.command_plan(current).and_then(|plan| plan.next) else {
                break;
            };
            let Some(entity) = self.node_state(next).command_entity.as_ref() else {
                break;
            };
            if entity.powered || entity.automatic {
                if !self.execute_command(next, host)? {
                    break;
                }
            }
            current = next;
            may_continue = true;
        }
        if matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::CommandBlock { repeating: true, .. })
        ) && self
            .node_state(node)
            .command_entity
            .as_ref()
            .is_some_and(|entity| entity.powered || entity.automatic)
        {
            self.schedule_half_tick(node, 1, TickPriority::Normal);
        }
        Ok(())
    }

    fn execute_command<'h>(&mut self, node: NodeId, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<bool, GraphExecutionError> {
        let position = self.position(node);
        let Some(mut entity) = self.node_state(node).command_entity.clone() else {
            return Ok(false);
        };
        if host.is_none() {
            return Err(GraphExecutionError::CommandHostUnavailable { position });
        }
        let tick = self.future.clock.logical_tick.min(i64::MAX as u64) as i64;
        if entity.update_last_execution && entity.last_execution == tick {
            return Ok(false);
        }
        if matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::CommandBlock { chain: true, .. })
        ) {
            entity.condition_met = self.command_condition_met(node);
        }
        entity.success_count = 0;
        if entity.condition_met && !entity.command.trim().is_empty() {
            let source = crate::redstone::command_block::command_source(&entity.custom_name);
            match host
                .as_mut()
                .expect("host was checked before command execution")
                .run_command(&entity.command, &source)
            {
                Ok(()) => {
                    entity.success_count = 1;
                    if entity.track_output {
                        entity.last_output = Some(serde_json::json!({"text":crate::messages::COMMAND_BLOCK_EXECUTED}).to_string());
                    }
                }
                Err(error) => {
                    if entity.track_output {
                        entity.last_output = Some(serde_json::json!({"text":error,"color":"red"}).to_string());
                    }
                }
            }
        }
        if entity.update_last_execution {
            entity.last_execution = tick;
        }
        let success_count = entity.success_count.clamp(0, 15) as u8;
        let Some(ComponentState::Electrical(state)) = self.state.components.get_mut(node.index()) else {
            return Err(GraphExecutionError::UnsupportedComponent {
                position: Some(position),
                kind: "command block state",
            });
        };
        state.powered = entity.powered;
        state.output_strength = success_count;
        state.command_entity = Some(entity.clone());
        self.dirty[node.index()] = true;
        host.as_mut()
            .expect("host was checked before command execution")
            .publish_command_entity(position, BlockEntity::CommandBlock(entity));
        self.fire_route(node)?;
        Ok(true)
    }

    fn update_note(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let powered = self.input_strength(node, InputPort::Main) > 0;
        if powered == self.node_state(node).powered {
            return Ok(());
        }
        let plan = self
            .program
            .note_plan(node)
            .ok_or_else(|| self.unsupported(node, "noteblock compiled sites"))?;
        let instrument = mchprs_blocks::blocks::Instrument::from_block_below(
            self.resolve_site(plan.below, None)
                .ok_or_else(|| self.unsupported(node, "noteblock support site"))?
                .block,
        );
        let unblocked = self.resolve_site(plan.above, None).is_some_and(|site| site.block == Block::Air);
        let CircuitNodeKind::Electrical(NodeType::NoteBlock { note, .. }) = &self.program.nodes()[node.index()].kind else {
            return Err(self.unsupported(node, "invalid noteblock binding"));
        };
        let Some(ComponentState::Electrical(state)) = self.state.components.get_mut(node.index()) else {
            return Err(self.unsupported(node, "noteblock state"));
        };
        state.powered = powered;
        state.note_instrument = Some(instrument);
        self.dirty[node.index()] = true;
        if powered && unblocked {
            if let Some(pitch) = crate::redstone::noteblock::pitch(*note) {
                self.effects.push(PresentationEffect::Sound {
                    position: self.position(node),
                    id: instrument.to_sound_id(),
                    category: 2,
                    volume: 3.0,
                    pitch,
                });
            }
        }
        Ok(())
    }

    fn update_observer(&mut self, node: NodeId, direction: Option<BlockFace>, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        let facing = match self.program.nodes()[node.index()].kind {
            CircuitNodeKind::Observer { facing, .. } => facing,
            _ => unreachable!(),
        };
        let powered = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Electrical { powered, .. } => Some(powered),
                _ => None,
            })
            .unwrap_or(self.node_state(node).powered);
        let qualifies = match direction {
            Some(direction) => facing == direction.into(),
            None => powered,
        };
        if qualifies && !self.is_pending(node) {
            self.schedule(node, 1, TickPriority::Normal);
        }
        Ok(())
    }

    fn tick_observer(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let powered = self.node_state(node).powered;
        if !powered {
            self.schedule(node, 1, TickPriority::Normal);
        }
        self.set_node(node, !powered, u8::from(!powered) * 15, None, true)
    }

    fn update_torch(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        let lit = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Electrical { powered, .. } => Some(powered),
                _ => None,
            })
            .unwrap_or(self.node_state(node).powered);
        if lit == (self.input_strength(node, InputPort::Main) > 0) && !self.is_pending(node) {
            self.schedule(node, 1, TickPriority::Normal);
        }
        Ok(())
    }

    fn tick_torch(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let should_be_off = self.input_strength(node, InputPort::Main) > 0;
        let lit = self.node_state(node).powered;
        if lit == should_be_off {
            self.set_node(node, !lit, u8::from(!lit) * 15, None, true)?;
        }
        Ok(())
    }

    fn update_repeater(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        let current = self.node_state(node);
        let (powered, locked) = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Electrical { powered, locked } => Some((powered, locked)),
                _ => None,
            })
            .unwrap_or((current.powered, current.repeater_locked));
        let should_lock = self.repeater_locked(node);
        if locked != should_lock {
            self.set_node(node, powered, u8::from(powered) * 15, Some(should_lock), false)?;
        }
        if !should_lock && !self.is_pending(node) {
            let should_power = self.input_strength(node, InputPort::Main) > 0;
            if should_power != powered {
                let facing_diode = matches!(
                    self.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::Repeater { facing_diode: true, .. })
                );
                let priority = if facing_diode {
                    TickPriority::Highest
                } else if !should_power {
                    TickPriority::Higher
                } else {
                    TickPriority::High
                };
                self.schedule(node, self.repeater_delay(node), priority);
            }
        }
        Ok(())
    }

    fn tick_repeater(&mut self, node: NodeId, delay: u8) -> Result<(), GraphExecutionError> {
        if self.node_state(node).repeater_locked {
            return Ok(());
        }
        let powered = self.node_state(node).powered;
        let should_power = self.input_strength(node, InputPort::Main) > 0;
        if powered && !should_power {
            self.set_node(node, false, 0, None, true)?;
        } else if !powered {
            if !should_power {
                self.schedule(node, delay as usize, TickPriority::Higher);
            }
            self.set_node(node, true, 15, None, true)?;
        }
        Ok(())
    }

    fn repeater_delay(&self, node: NodeId) -> usize {
        match self.program.nodes()[node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, .. }) => delay as usize,
            _ => unreachable!(),
        }
    }

    fn repeater_locked(&self, node: NodeId) -> bool {
        self.input_strength(node, InputPort::Side) > 0
    }

    fn update_comparator(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        #[cfg(test)]
        if self.test_recording {
            let position = self.position(node);
            self.test_recorder.comparator_updates.push(position);
        }
        if self.is_pending(node) {
            return Ok(());
        }
        let strength = self.comparator_output(node);
        let powered = self.comparator_powered(node);
        let current = self.node_state(node);
        if strength != current.output_strength || powered != current.powered {
            let facing_diode = matches!(
                self.program.nodes()[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Comparator { facing_diode: true, .. })
            );
            self.schedule(node, 1, if facing_diode { TickPriority::High } else { TickPriority::Normal });
        }
        Ok(())
    }

    fn tick_comparator(&mut self, node: NodeId, mode: ComparatorMode, _facing_diode: bool) -> Result<(), GraphExecutionError> {
        let strength = self.comparator_output(node);
        #[cfg(test)]
        if self.test_recorder.comparator_tick_focus == Some((node, self.future.clock.logical_tick)) {
            let fixed_operands = |port| {
                self.program
                    .fixed_signal_query(PowerTarget::Input { node, port })
                    .map(|(constant, inputs)| {
                        let inputs = inputs
                            .iter()
                            .map(|input| {
                                let strength = match &self.state.components[input.node.index()] {
                                    ComponentState::Electrical(state) => state.output_strength,
                                    ComponentState::Wire(_) => self.wire_state(input.node).power,
                                    ComponentState::Specialized => 0,
                                };
                                let shape_ok = !input.require_cross || !crate::redstone::wire::is_dot(self.wire_state(input.node));
                                serde_json::json!({
                                    "node":input.node.index(),
                                    "position":self.program.nodes()[input.node.index()].position,
                                    "kind":format!("{:?}", self.program.nodes()[input.node.index()].kind),
                                    "strength":strength,
                                    "attenuation":input.attenuation,
                                    "require_cross":input.require_cross,
                                    "shape_ok":shape_ok,
                                    "contribution":if shape_ok { strength.saturating_sub(input.attenuation) } else { 0 },
                                })
                            })
                            .collect::<Vec<_>>();
                        serde_json::json!({"constant":constant,"inputs":inputs})
                    })
            };
            println!(
                "CHUNGUS TICK512 COMPARATOR HANDLER {:?} tick={} phase={:?} main={} side={} calculated={} stored={} powered={} main_operands={:?} side_operands={:?}",
                self.position(node),
                self.future.clock.logical_tick,
                self.future.clock.phase,
                self.comparator_input(node),
                self.comparator_side(node),
                strength,
                self.node_state(node).output_strength,
                self.comparator_powered(node),
                fixed_operands(InputPort::Main),
                fixed_operands(InputPort::Side),
            );
        }
        let current = self.node_state(node);
        if strength != current.output_strength || mode == ComparatorMode::Compare {
            let powered = self.comparator_powered(node);
            self.set_node(node, powered, strength, None, false)?;
            self.fire_route(node)?;
        }
        Ok(())
    }

    fn comparator_input(&self, node: NodeId) -> u8 {
        let Some(plan) = self.program.comparator_input(node) else {
            return 0;
        };
        if let Some(source) = plan.direct_override {
            return self.override_strength(source);
        }
        let rear = self.input_strength(node, InputPort::Main);
        if rear >= plan.far_threshold {
            return rear;
        }
        if let Some((_, source, guard)) = plan.far_override
            && self.guard_matches(guard, None)
        {
            return self.override_strength(source);
        }
        rear
    }

    fn comparator_side(&self, node: NodeId) -> u8 {
        self.input_strength(node, InputPort::Side)
    }

    fn comparator_output(&self, node: NodeId) -> u8 {
        let input = self.comparator_input(node);
        let side = self.comparator_side(node);
        let mode = match self.program.nodes()[node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::Comparator { mode, .. }) => mode,
            _ => unreachable!(),
        };
        crate::redpiler::power_rules::comparator_output_strength(input, side, mode)
    }

    fn comparator_powered(&self, node: NodeId) -> bool {
        let input = self.comparator_input(node);
        let side = self.comparator_side(node);
        let mode = match self.program.nodes()[node.index()].kind {
            CircuitNodeKind::Electrical(NodeType::Comparator { mode, .. }) => mode,
            _ => unreachable!(),
        };
        crate::redpiler::power_rules::comparator_is_powered(input, side, mode)
    }

    fn override_strength(&self, value: ComparatorOverride) -> u8 {
        match value {
            ComparatorOverride::Literal(strength) => strength,
            ComparatorOverride::LiveComponentOutput(node) => self.node_state(node).output_strength,
        }
    }

    fn update_lamp(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        let lit = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Electrical { powered, .. } => Some(powered),
                _ => None,
            })
            .unwrap_or(self.node_state(node).powered);
        let should_lit = self.input_strength(node, InputPort::Main) > 0;
        if lit && !should_lit {
            if !self.is_pending(node) {
                self.schedule(node, 2, TickPriority::Normal);
            }
        } else if !lit && should_lit {
            self.set_node(node, true, 0, None, false)?;
        }
        Ok(())
    }

    fn update_hopper(&mut self, node: NodeId, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        let powered = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Electrical { powered, .. } => Some(powered),
                _ => None,
            })
            .unwrap_or(self.node_state(node).powered);
        let should_power = self.input_strength(node, InputPort::Main) > 0;
        if powered != should_power {
            // Native hopper updates change enabled without notifying neighbors or touching inventory.
            self.set_node(node, should_power, self.node_state(node).output_strength, None, false)?;
        }
        Ok(())
    }

    fn tick_lamp(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        if self.node_state(node).powered && self.input_strength(node, InputPort::Main) == 0 {
            self.set_node(node, false, 0, None, false)?;
        }
        Ok(())
    }

    fn update_bulb(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let input = self.input_strength(node, InputPort::Main) > 0;
        let current = self.node_state(node);
        if current.powered != input {
            let lit = if input {
                !current.output_strength.gt(&0)
            } else {
                current.output_strength > 0
            };
            if input {
                let position = self.position(node);
                self.effects.push(PresentationEffect::Sound {
                    position,
                    id: crate::sound::event_id(if lit {
                        "block.copper_bulb.turn_on"
                    } else {
                        "block.copper_bulb.turn_off"
                    }),
                    category: 4,
                    volume: 1.0,
                    pitch: 1.0,
                });
            }
            self.set_node(node, input, if lit { 15 } else { 0 }, None, false)?;
            self.fire_route(node)?;
        }
        Ok(())
    }

    fn node_state(&self, node: NodeId) -> &NodeState {
        match &self.state.components[node.index()] {
            ComponentState::Electrical(state) => state,
            _ => unreachable!("component state requested for a specialized graph node"),
        }
    }

    /// Send a node's output notifications, if it has an output route.
    fn fire_route(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        if let Some(program) = &self.fixed_classic {
            let route = program.nodes[node.index()].output_route;
            return route.map_or(Ok(()), |route| self.fixed_classic_route(route, None));
        }
        let route = self.program.nodes()[node.index()].output_route;
        if let Some(route) = route {
            self.deliver_route(route, None)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    fn has_semantic_callback(&self, receiver: CallbackReceiver) -> bool {
        match receiver {
            CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) => true,
            CallbackReceiver::Component(node) => self.program.nodes()[node.index()].block.is_some_and(|block| {
                matches!(
                    block.get_name(),
                    "redstone_wire"
                        | "redstone_torch"
                        | "redstone_wall_torch"
                        | "redstone_repeater"
                        | "redstone_comparator"
                        | "redstone_lamp"
                        | "observer"
                        | "copper_bulb"
                        | "hopper"
                        | "note_block"
                        | "iron_trapdoor"
                )
            }),
            CallbackReceiver::Contact(_) => false,
        }
    }

    /// Run a route in delivery order, using a specialized plan if one is installed.
    /// The general path keeps duplicate notifications. A callback can run another route.
    #[inline(always)]
    fn deliver_route(&mut self, route: RouteId, captured: Option<&[Option<ObservedSite>]>) -> Result<(), GraphExecutionError> {
        if self.fixed_classic.is_some() && !self.logical_node_owned(self.program.routes()[route.index()].source) {
            return self.fixed_classic_route(route, captured);
        }
        let result = self.deliver_route_inner(route, captured);
        result
    }

    fn deliver_route_inner(&mut self, route: RouteId, captured: Option<&[Option<ObservedSite>]>) -> Result<(), GraphExecutionError> {
        let retained_frame = self.logical_node_owned(self.program.routes()[route.index()].source);
        if retained_frame
            && self
                .active_frame_retained_routes
                .binary_search_by_key(&route, |occurrence| occurrence.route)
                .is_err()
        {
            return Ok(());
        }
        let has_logical_sample = self.logical_bud_samples_by_route.get(route.index()).is_some_and(Option::is_some);
        let has_logical_firing = self.logical_firing.as_ref().is_some_and(|runtime| runtime.owns_route(route));
        if !retained_frame
            && !has_logical_sample
            && !has_logical_firing
            && let Some(kernel) = self.instant_kernel.active.clone()
            && let Ok(index) = kernel.routes.binary_search_by_key(&route, |row| row.route)
        {
            return self.deliver_kernel_route(&kernel.routes[index]);
        }
        if !retained_frame
            && !has_logical_sample
            && !has_logical_firing
            && let Some(binding) = self.program.instant_route_binding(route)
        {
            return self.deliver_instant_route(route, binding, captured);
        }
        let delivery_count = self.program.routes()[route.index()].deliveries.len();
        let classic_range = self.program.classic().route_ranges.get(route.index()).cloned().unwrap_or(0..0);
        let mut classic_cursor = classic_range.start;
        for index in 0..delivery_count {
            if !retained_frame && classic_cursor < classic_range.end {
                let binding = self.program.classic().route_entries[classic_cursor];
                if binding.ordinal as usize == index {
                    classic_cursor += 1;
                    if self.run_classic_entry(binding.entry as usize) {
                        continue;
                    }
                }
            }
            let delivery = self.program.routes()[route.index()].deliveries[index];
            let occurrence = RouteOccurrence {
                route,
                ordinal: index as u32,
            };
            if retained_frame
                && self
                    .active_frame_retained_routes
                    .binary_search_by_key(&(route, occurrence.ordinal), |row| (row.route, row.ordinal))
                    .is_err()
            {
                continue;
            }
            self.deliver_callback(route, delivery, captured, Some(occurrence))?;
        }
        Ok(())
    }

    /// Deliver one original callback occurrence bound to a logical BUD transition cut.
    /// Direct self-samples pass back through the owner sampler; external deliveries stay Graph-owned.
    pub(super) fn deliver_logical_bud_occurrence(
        &mut self,
        actor: PistonId,
        occurrence: RouteOccurrence,
        dispatch: LogicalBudDispatch,
    ) -> Result<(), GraphExecutionError> {
        #[cfg(test)]
        if self.test_recorder.test_recording {
            self.test_recorder.logical_bud_occurrence_dispatches += 1;
        }
        if self.program.logical_bud_owner(actor).is_none() {
            return Ok(());
        }
        let Some(delivery) = self
            .program
            .routes()
            .get(occurrence.route.index())
            .and_then(|route| route.deliveries.get(occurrence.ordinal as usize))
            .copied()
        else {
            return Err(GraphExecutionError::UnsupportedComponent {
                position: Some(self.program.pistons()[actor.index()].position),
                kind: "invalid logical BUD route occurrence",
            });
        };
        let classic_range = self.program.classic().route_ranges.get(occurrence.route.index()).cloned().unwrap_or(0..0);
        if let Some(binding) = self
            .program
            .classic()
            .route_entries
            .get(classic_range)
            .unwrap_or(&[])
            .iter()
            .find(|binding| binding.ordinal == occurrence.ordinal)
            .copied()
            && self.run_classic_entry(binding.entry as usize)
        {
            return Ok(());
        }
        match dispatch {
            LogicalBudDispatch::OwnSample => {
                debug_assert!(
                    self.program
                        .logical_bud_owner(actor)
                        .is_some_and(|plan| plan.sample_routes.contains(&occurrence))
                );
                #[cfg(test)]
                if self.test_recorder.callback_census_enabled
                    && let Some(receiver) = delivery.receiver
                {
                    self.record_callback_candidate(receiver);
                }
                if matches!(delivery.kind, DeliveryKind::Update { .. }) && delivery.receiver.is_some_and(|receiver| self.instant_event_noop(receiver))
                {
                    return Ok(());
                }
                if delivery.view != OperandView::Live || !self.guard_matches(delivery.guard, None) {
                    return Ok(());
                }
                #[cfg(test)]
                if self.test_recorder.test_recording {
                    *self.test_recorder.logical_bud_direct_samples.entry(actor).or_default() += 1;
                }
                self.sample_piston_from(actor, occurrence.route)
            }
            LogicalBudDispatch::Boundary => {
                #[cfg(test)]
                if self.test_recorder.test_recording {
                    *self.test_recorder.logical_bud_boundary_callbacks.entry(actor).or_default() += 1;
                }
                self.deliver_callback(occurrence.route, delivery, None, Some(occurrence))
            }
        }
    }

    /// Replay the retained boundary rows for a transition cut in their compiled order.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn deliver_logical_bud_cut(
        &mut self,
        actor: PistonId,
        cut: LogicalBudEventCut,
        resource_moved: bool,
        interrupted_head: bool,
        restore_matching_extension: bool,
        pulled_resource: bool,
        actor_revision_current: bool,
        resource_revision_current: bool,
    ) -> Result<(), GraphExecutionError> {
        let event_count = self.program.logical_bud_owner(actor).map_or(0, |plan| plan.transition.events.len());
        for index in 0..event_count {
            let event = self
                .program
                .logical_bud_owner(actor)
                .and_then(|plan| plan.transition.events.get(index))
                .copied();
            let Some(event) = event else { break };
            if event.cut != cut {
                continue;
            }
            let should_deliver = match event.condition {
                LogicalBudEventCondition::Always => true,
                LogicalBudEventCondition::ResourceMoved => resource_moved,
                LogicalBudEventCondition::InterruptedHead => interrupted_head,
                LogicalBudEventCondition::NotInterruptedHead => !interrupted_head,
                LogicalBudEventCondition::RestoreMatchingExtension => restore_matching_extension,
                LogicalBudEventCondition::PulledResource => pulled_resource,
                LogicalBudEventCondition::ActorRevisionCurrent => actor_revision_current,
                LogicalBudEventCondition::ResourceRevisionCurrent => resource_revision_current,
            };
            if should_deliver {
                let redundant = self.logical_bud_event_base_pending(actor, event).is_some() && {
                    #[cfg(test)]
                    {
                        !self.test_recorder.disable_busy_callback_contraction
                    }
                    #[cfg(not(test))]
                    {
                        true
                    }
                };
                if redundant {
                    // The owner sampler itself returns before reading power or enqueuing work while its base is pending.
                    #[cfg(test)]
                    {
                        if self.test_recorder.test_recording {
                            self.test_recorder.logical_bud_busy_callbacks_elided += 1;
                        }
                    }
                    continue;
                }
                self.deliver_logical_bud_occurrence(actor, event.occurrence, event.dispatch)?;
            }
        }
        Ok(())
    }

    fn logical_bud_event_base_pending(&self, actor: PistonId, event: LogicalBudRouteEvent) -> Option<PistonId> {
        let delivery = *self
            .program
            .routes()
            .get(event.occurrence.route.index())?
            .deliveries
            .get(event.occurrence.ordinal as usize)?;
        if delivery.view != OperandView::Live {
            return None;
        }
        let target = match event.dispatch {
            LogicalBudDispatch::OwnSample => actor,
            LogicalBudDispatch::Boundary => self
                .logical_bud_samples_by_route
                .get(event.occurrence.route.index())?
                .as_ref()?
                .get(event.occurrence.ordinal as usize)
                .copied()??,
        };
        let receiver = delivery
            .receiver
            .or_else(|| match (self.program.sites()[delivery.target.index()].source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            })?;
        let matches_target = match receiver {
            CallbackReceiver::PistonBase(owner) | CallbackReceiver::PistonHead(owner) => owner == target,
            CallbackReceiver::Component(node) => self.program.pistons()[target.index()].node == node,
            CallbackReceiver::Contact(_) => false,
        };
        if !matches_target {
            return None;
        }
        if matches!(receiver, CallbackReceiver::Component(node) if self.logical_firing.as_ref().is_some_and(|runtime| runtime.owns_observer(node))) {
            return None;
        }
        self.logical_bud_owner_by_actor.get(target.index())?.as_ref()?;
        if !self.state.pistons[target.index()].availability.base_pending() || self.classic_route_entry(event.occurrence).is_some() {
            return None;
        }
        Some(target)
    }

    fn classic_route_entry(&self, occurrence: RouteOccurrence) -> Option<ClassicRouteEntry> {
        let range = self.program.classic().route_ranges.get(occurrence.route.index())?;
        self.program
            .classic()
            .route_entries
            .get(range.clone())?
            .iter()
            .find(|binding| binding.ordinal == occurrence.ordinal)
            .copied()
    }

    /// Check a delivery's guard, then notify its receiver using live or captured state.
    /// Skip it if the guard is false or a required capture is missing.
    #[inline(always)]
    fn deliver_callback(
        &mut self,
        route: RouteId,
        delivery: RouteDelivery,
        captured: Option<&[Option<ObservedSite>]>,
        occurrence: Option<RouteOccurrence>,
    ) -> Result<(), GraphExecutionError> {
        #[cfg(test)]
        if self.test_recorder.callback_census_enabled
            && let Some(receiver) = delivery.receiver
        {
            self.record_callback_candidate(receiver);
        }
        if matches!(delivery.kind, DeliveryKind::Update { .. }) && delivery.receiver.is_some_and(|receiver| self.instant_event_noop(receiver)) {
            return Ok(());
        }
        if !self.guard_matches(delivery.guard, captured) {
            return Ok(());
        }
        let observed = if delivery.view == OperandView::Captured {
            captured.and_then(|sites| sites.get(delivery.target.index())).and_then(Option::as_ref)
        } else {
            None
        };
        if delivery.view == OperandView::Captured && observed.is_none() {
            return Ok(());
        }
        let receiver = delivery
            .receiver
            .or_else(|| match (self.program.sites()[delivery.target.index()].source, delivery.kind) {
                (SiteSource::Component(node), DeliveryKind::Update { .. }) => Some(CallbackReceiver::Component(node)),
                _ => None,
            });
        let Some(receiver) = receiver else {
            return Ok(());
        };
        if !self.logical_device_owner.is_empty() && !self.logical_node_owned(self.program.routes()[route.index()].source) {
            let target = match receiver {
                CallbackReceiver::Component(node) => node,
                CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => self.program.pistons()[actor.index()].node,
                CallbackReceiver::Contact(contact) => self.program.contacts()[contact.index()].node,
            };
            if self.logical_node_owned(target) {
                return Err(self.unsupported(target, "ordinary callback crosses a logical-device scope"));
            }
        }
        if matches!(receiver, CallbackReceiver::Component(node) if self.logical_firing.as_ref().is_some_and(|runtime| runtime.owns_observer(node))) {
            if matches!(delivery.kind, DeliveryKind::Update { .. } | DeliveryKind::Shape { .. }) {
                let node = match receiver {
                    CallbackReceiver::Component(node) => node,
                    _ => unreachable!(),
                };
                self.update_component(node, delivery.kind, observed)?;
            }
            return Ok(());
        }
        let logical_sample = occurrence
            .and_then(|occurrence| {
                self.logical_bud_samples_by_route
                    .get(occurrence.route.index())?
                    .as_ref()?
                    .get(occurrence.ordinal as usize)
            })
            .copied()
            .flatten()
            .filter(|&actor| match receiver {
                CallbackReceiver::PistonBase(id) | CallbackReceiver::PistonHead(id) => id == actor,
                CallbackReceiver::Component(node) => self.program.pistons()[actor.index()].node == node,
                CallbackReceiver::Contact(_) => false,
            });
        if let Some(actor) = logical_sample {
            self.sample_piston_from(actor, route)?;
            return Ok(());
        }
        let owned_firing_actor = self.logical_firing.as_ref().and_then(|runtime| {
            let actor = runtime.actor();
            match receiver {
                CallbackReceiver::PistonBase(candidate) | CallbackReceiver::PistonHead(candidate) if candidate == actor => Some(actor),
                CallbackReceiver::Component(node) if self.program.pistons()[actor.index()].node == node => Some(actor),
                _ => None,
            }
        });
        if let Some(actor) = owned_firing_actor {
            self.sample_logical_firing_actor(actor, route)?;
            return Ok(());
        }
        self.counts.callbacks += 1;
        #[cfg(test)]
        if self.test_recorder.callback_census_enabled {
            self.record_accepted_callback(delivery.kind, receiver);
        }
        #[cfg(test)]
        self.record_route_event(route, delivery, receiver);
        #[cfg(not(test))]
        let _ = route;
        match receiver {
            CallbackReceiver::Component(node) => self.update_component(node, delivery.kind, observed)?,
            CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => {
                if self.classic_proof.is_some() {
                    return Err(GraphExecutionError::UnsupportedComponent {
                        position: Some(self.program.sites()[delivery.target.index()].position),
                        kind: "classic proof piston callback",
                    });
                }
                self.sample_piston_from(piston, route)?;
            }
            CallbackReceiver::Contact(_) => {
                if self.classic_proof.is_some() {
                    return Err(GraphExecutionError::UnsupportedComponent {
                        position: Some(self.program.sites()[delivery.target.index()].position),
                        kind: "classic proof contact callback",
                    });
                }
            }
        }
        Ok(())
    }

    /// The test oracle projects absorbed internal notifications; counters record actual dispatch only.
    #[cfg(test)]
    fn record_route_event(&mut self, route: RouteId, delivery: RouteDelivery, receiver: CallbackReceiver) {
        if self.test_recording && self.has_semantic_callback(receiver) && matches!(delivery.kind, DeliveryKind::Update { .. }) {
            self.test_recorder.observed_operations.push(GraphObservedOperation::Callback {
                target: self.program.sites()[delivery.target.index()].position,
                direction: match delivery.kind {
                    DeliveryKind::Update { direction } => direction,
                    DeliveryKind::Shape { direction } => Some(direction),
                },
            });
        }
        graph_test_trace!(self, format!("route {}:{} {:?}", route.index(), delivery.sequence, delivery.kind));
    }

    fn set_node(&mut self, node: NodeId, powered: bool, output_strength: u8, locked: Option<bool>, notify: bool) -> Result<(), GraphExecutionError> {
        let ComponentState::Electrical(state) = &mut self.state.components[node.index()] else {
            return Err(self.unsupported(node, "specialized component state"));
        };
        let changed =
            state.powered != powered || state.output_strength != output_strength || locked.is_some_and(|locked| state.repeater_locked != locked);
        if self.canonical_transport_repeaters
            && matches!(
                self.program.nodes()[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Repeater { .. })
            )
            && (state.output_strength != 15 * u8::from(state.powered) || output_strength != 15 * u8::from(powered))
        {
            self.canonical_transport_repeaters = false;
        }
        state.powered = powered;
        state.output_strength = output_strength;
        state.pending_tick = false;
        if let Some(locked) = locked {
            state.repeater_locked = locked;
        }
        if changed {
            self.dirty[node.index()] = true;
            #[cfg(test)]
            graph_test_trace!(self, format!("state {:?} powered={powered} strength={output_strength}", node));
            if notify {
                self.fire_route(node)?;
            }
        }
        Ok(())
    }

    fn unsupported(&self, node: NodeId, kind: &'static str) -> GraphExecutionError {
        GraphExecutionError::UnsupportedComponent {
            position: self.program.nodes()[node.index()].position,
            kind,
        }
    }

    fn position(&self, node: NodeId) -> BlockPos {
        self.program.nodes()[node.index()].position.expect("component has a position")
    }

    fn is_bulb(&self, node: NodeId) -> bool {
        matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Electrical(NodeType::CopperBulb))
    }

    pub(crate) fn execution_counts(&self) -> GraphExecutionCounts {
        self.counts
    }

    pub(crate) fn node_count(&self) -> usize {
        self.program.nodes().len()
    }

    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(BlockPos, u8)> {
        self.program
            .nodes()
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                if !matches!(
                    node.kind,
                    CircuitNodeKind::Electrical(NodeType::Torch | NodeType::Repeater { .. } | NodeType::Comparator { .. })
                ) {
                    return None;
                }
                let ComponentState::Electrical(state) = &self.state.components[index] else {
                    return None;
                };
                let strength = state.output_strength;
                Some((node.position?, strength))
            })
            .collect()
    }

    #[cfg(test)]
    fn trace(&self) -> &[String] {
        &self.trace
    }

    #[cfg(test)]
    pub(crate) fn clear_test_traces(&mut self) {
        self.test_recorder.clear();
    }

    #[cfg(test)]
    pub(crate) fn native_piston_trace(&self) -> &[crate::redstone::piston::trace::Entry] {
        &self.test_recorder.native_piston_trace
    }

    #[cfg(test)]
    pub(crate) fn disable_logical_bud_owners_for_test(&mut self) {
        self.logical_bud_owner_by_actor.fill(None);
        self.logical_bud_samples_by_route.fill(None);
    }

    #[cfg(test)]
    pub(crate) fn disable_busy_callback_contraction_for_test(&mut self) {
        self.test_recorder.disable_busy_callback_contraction = true;
    }

    #[cfg(test)]
    pub(crate) fn install_accepted_boundary_probe(&mut self, actor: PistonId, circuit: SettledCircuit) -> Result<(), String> {
        self.test_recorder.accepted_boundary_probe = Some(accepted_boundary_probe::Probe::new(actor, &self.program, circuit)?);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn take_accepted_boundary_probe(&mut self) -> Option<accepted_boundary_probe::Report> {
        self.test_recorder
            .accepted_boundary_probe
            .take()
            .map(accepted_boundary_probe::Probe::into_report)
    }

    #[cfg(test)]
    pub(crate) fn set_test_recording(&mut self, enabled: bool) {
        self.test_recorder.test_recording = enabled;
        if !enabled {
            self.clear_test_traces();
        }
    }

    #[cfg(test)]
    pub(crate) fn callback_census(&self) -> CallbackCensus {
        self.test_recorder.callback_census.clone()
    }

    #[cfg(test)]
    pub(crate) fn enable_callback_census(&mut self) {
        self.test_recorder.callback_census_enabled = true;
    }

    #[cfg(test)]
    pub(crate) fn enable_piston_sample_census(&mut self) {
        self.test_recorder.piston_sample_census.enable();
    }

    #[cfg(test)]
    pub(crate) fn piston_sample_census(&self) -> serde_json::Value {
        self.test_recorder.piston_sample_census.to_json()
    }

    #[cfg(test)]
    pub(crate) fn enable_value_region_census(&mut self) {
        self.test_recorder.value_region_census_enabled = true;
        let metadata = self.program.transport_census_metadata();
        let mut rows = Vec::with_capacity(metadata.len());
        for net in 0..metadata.len() {
            let count = self.program.instant_value_region_net(net as u32).map_or(0, |plan| plan.regions.len());
            rows.push(vec![ValueRegionExecutionRow::default(); count]);
        }
        self.test_recorder.value_region_census.by_net = rows;
    }

    #[cfg(test)]
    pub(crate) fn value_region_census(&self) -> ValueRegionExecutionCensus {
        self.test_recorder.value_region_census.clone()
    }

    #[cfg(test)]
    pub(super) fn record_value_region_visit(&mut self, net: usize, region: u32, inert: bool) {
        if let Some(row) = self
            .test_recorder
            .value_region_census
            .by_net
            .get_mut(net)
            .and_then(|rows| rows.get_mut(region as usize))
        {
            if inert {
                row.inert_visits += 1;
            } else {
                row.dense_visits += 1;
            }
        }
    }

    #[cfg(test)]
    pub(super) fn record_value_region_write(&mut self, net: usize, region: u32) {
        if let Some(row) = self
            .test_recorder
            .value_region_census
            .by_net
            .get_mut(net)
            .and_then(|rows| rows.get_mut(region as usize))
        {
            row.changed_wire_writes += 1;
        }
    }

    #[cfg(test)]
    pub(crate) fn transport_census_metadata(&self) -> Vec<serde_json::Value> {
        self.program.transport_census_metadata()
    }

    #[cfg(test)]
    pub(crate) fn value_region_census_program_net(&self, net: u32) -> Option<(&TransportNet, &InstantValueRegionNet)> {
        Some((
            self.program.instant_transport_net(net).as_ref(),
            self.program.instant_value_region_net(net)?.as_ref(),
        ))
    }

    #[cfg(test)]
    pub(crate) fn value_region_census_drive_program(&self) -> &InstantDriveProgram {
        self.program.instant_drive_program()
    }

    #[cfg(test)]
    pub(crate) fn value_region_census_node_position(&self, node: NodeId) -> Option<BlockPos> {
        self.program.nodes().get(node.index()).and_then(|spec| spec.position)
    }

    #[cfg(test)]
    pub(crate) fn value_region_census_site_position(&self, site: SiteId) -> Option<BlockPos> {
        self.program.sites().get(site.index()).map(|spec| spec.position)
    }

    #[cfg(test)]
    pub(crate) fn value_region_census_route(&self, route: RouteId) -> &Route {
        &self.program.routes()[route.index()]
    }

    #[cfg(test)]
    fn scheduled_ticks(&self) -> Vec<TickEntry> {
        self.future
            .scheduler
            .iter()
            .map(|(tick, delay, priority)| TickEntry {
                pos: tick.position,
                block_type: tick.block_type,
                ticks_left: delay as u32,
                tick_priority: priority,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "graph/tests/components.rs"]
mod tests;

#[cfg(test)]
#[path = "graph/tests/coalesce_delayed_updates.rs"]
mod coalesce_delayed_update_tests;

#[cfg(test)]
#[path = "graph/tests/fuzz.rs"]
mod fuzz_tests;

#[cfg(test)]
#[path = "graph/tests/test_family.rs"]
mod test_family_tests;

#[cfg(test)]
#[path = "graph/tests/adder.rs"]
mod adder_tests;

#[cfg(test)]
#[path = "graph/tests/hopper.rs"]
mod hopper_tests;

#[cfg(test)]
#[path = "graph/tests/wire_query.rs"]
mod wire_query_tests;

#[cfg(test)]
#[path = "graph/tests/fixed_signals.rs"]
mod fixed_signal_tests;

#[cfg(test)]
#[path = "graph/tests/classic_pending.rs"]
mod classic_pending_tests;

#[cfg(test)]
#[path = "graph/tests/isolated_wire.rs"]
mod isolated_wire_tests;

#[cfg(test)]
#[path = "graph/tests/instant_reset.rs"]
mod instant_reset_tests;

#[cfg(test)]
#[path = "graph/tests/wire_callbacks.rs"]
mod wire_callback_tests;

#[cfg(test)]
#[path = "graph/tests/fixed_classic_edges.rs"]
mod fixed_classic_edges;
#[cfg(test)]
#[path = "graph/tests/fixed_classic.rs"]
mod fixed_classic_tests;
