//! Graph-native execution for rich piston circuits. The World is read only at
//! compilation; flush and reset project graph state back to it.

mod fixed_point;
#[cfg(test)]
#[path = "graph/tests/unchecked_classic_anpu.rs"]
mod unchecked_classic_anpu;
#[cfg(test)]
#[path = "graph/tests/chungus_census.rs"]
mod chungus_census;
pub(crate) mod classic;
#[cfg(test)]
#[path = "graph/tests/fixed_point_cpu.rs"]
mod fixed_point_cpu_tests;
mod instant;
mod instant_proxy;
mod pistons;
mod power;
mod projection;
mod reset;
mod sites;
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
    test_recording: bool,
    trace: Vec<String>,
    callback_order: Vec<(BlockPos, DeliveryKind)>,
    comparator_updates: Vec<BlockPos>,
    power_links_examined: Cell<usize>,
    observed_operations: Vec<GraphObservedOperation>,
}

#[cfg(test)]
impl TestRecorder {
    fn clear(&mut self) {
        self.trace.clear();
        self.callback_order.clear();
        self.comparator_updates.clear();
        self.observed_operations.clear();
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

pub(crate) struct GraphBackend {
    program: CircuitProgram,
    state: CircuitState,
    positions: FxHashMap<BlockPos, NodeId>,
    component_sites: Vec<Option<SiteId>>,
    /// This pool reuses compiled hop scratch; each nested invocation owns a separate walk.
    wire_walks: Vec<WireWalk>,
    classic_proof: Option<classic::ProofRecorder>,
    #[cfg(test)]
    unchecked_classic: Option<std::sync::Arc<unchecked_classic_anpu::UncheckedProgram>>,
    #[cfg(test)]
    wire_tapes: Option<std::sync::Arc<unchecked_classic_anpu::TapeProgram>>,
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
    pub(crate) fn new(program: CircuitProgram, state: CircuitState, logical_tick: u64) -> Result<Self, GraphBackendError> {
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
        let limits = AnalysisLimits::for_budget(monitor.budget_multiplier());
        let mut prepared = passes::rich::prepare(world, bounds, ticks, &monitor, limits).map_err(map_preparation_error)?;
        let pipeline = passes::default_graph_pipeline(options);
        passes::run_graph_pipeline(&mut prepared.graph, options, &prepared.entry, &pipeline, &monitor).map_err(GraphBackendError::PassPipeline)?;
        prepared.finish_pipeline_statistics(&monitor);
        let lowered = prepared.lower(&monitor).map_err(GraphBackendError::Graph)?;
        Self::from_lowered(lowered, &monitor)
    }

    pub(crate) fn from_lowered(lowered: LoweredCircuit, monitor: &TaskMonitor) -> Result<Self, GraphBackendError> {
        Self::validate_supported(&lowered.program)?;
        let mut backend = Self::new_with_positions(lowered.program, lowered.state, lowered.logical_tick, lowered.positions)?;
        backend.import_ticks(lowered.imported_ticks)?;
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
        let (contacts_by_piston, contacts_by_resource, always_dirty_contacts) =
            projection::contact_projection_dependencies(&program);
        let contact_count = program.contacts().len();
        let full_contact_projection = program
            .contacts()
            .iter()
            .map(|contact| contact.position)
            .collect::<FxHashSet<_>>()
            .len()
            != contact_count;
        let contact_dirty = vec![true; contact_count];
        let dirty_contacts = (0..contact_count).map(|index| ContactId::new(index as u32)).collect();
        let projected_pistons = state.pistons.to_vec();
        let projected_resources = state.resources.to_vec();
        Ok(Self {
            program,
            state,
            positions,
            component_sites,
            wire_walks: Vec::new(),
            classic_proof: None,
            #[cfg(test)]
            unchecked_classic: None,
            #[cfg(test)]
            wire_tapes: None,
            future: GraphFuture {
                scheduler: TickScheduler::default(),
                pending: FxHashMap::default(),
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
        let tick = ScheduledNodeTick {
            node: Some(node),
            position: self.position(node),
            block_type: Some(self.registry_type(node)),
            registry_type: self.registry_type(node),
            current_registry_type: self.registry_type(node),
        };
        let key = tick.into();
        if self.future.pending.contains_key(&key) {
            return;
        }
        self.future.scheduler.schedule_half_tick(tick, delay, priority);
        self.future.pending.insert(key, 1);
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
        if let Some(count) = self.future.pending.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                self.future.pending.remove(&key);
            }
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), GraphExecutionError> {
        self.tick_with_host(None)
    }

    pub(crate) fn tick_with_world<W: World>(&mut self, world: &mut W) -> Result<(), GraphExecutionError> {
        self.tick_with_host(Some(world))
    }

    fn tick_with_host<'h>(&mut self, mut host: Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if host.is_none()
            && let Some(position) = self.command_host_preflight_position()
        {
            return Err(GraphExecutionError::CommandHostUnavailable { position });
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

    fn tick_phases<'h>(&mut self, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
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
        if let Some(error) = &self.failure { return Err(error.clone()); }
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
                self.set_node(node, powered, u8::from(powered) * 15, None, true)?;
            }
            CircuitNodeKind::Electrical(NodeType::Button) => {
                if !self.node_state(node).powered {
                    self.schedule(node, 10, TickPriority::Normal);
                    self.set_node(node, true, 15, None, true)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn set_pressure_plate(&mut self, position: BlockPos, powered: bool) -> Result<(), GraphExecutionError> {
        if let Some(error) = &self.failure { return Err(error.clone()); }
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
            self.set_node(node, powered, u8::from(powered) * 15, None, true)?;
        }
        Ok(())
    }

    fn latch_error(&mut self, result: &Result<(), GraphExecutionError>) {
        if let Err(error) = result {
            self.failure = Some(error.clone());
        }
    }

    fn tick_component<'h>(&mut self, node: NodeId, host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>) -> Result<(), GraphExecutionError> {
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

    fn update_component(&mut self, node: NodeId, kind: DeliveryKind, captured: Option<&ObservedSite>) -> Result<(), GraphExecutionError> {
        if self.classic_proof.is_some() && matches!(kind, DeliveryKind::Update { .. }) && !matches!(self.program.nodes()[node.index()].kind,CircuitNodeKind::Wire(_)) {
            return self.record_classic_sample(node,captured);
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
        let powered = self.input_strength(node, InputPort::Main) > 0;
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
            .publish_command_entity(position, BlockEntity::CommandBlock(Box::new(entity)));
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
        match mode {
            ComparatorMode::Compare => {
                if input >= side {
                    input
                } else {
                    0
                }
            }
            ComparatorMode::Subtract => input.saturating_sub(side),
        }
    }

    fn comparator_powered(&self, node: NodeId) -> bool {
        let input = self.comparator_input(node);
        let side = self.comparator_side(node);
        if input == 0 {
            false
        } else if input > side {
            true
        } else {
            input == side
                && matches!(
                    self.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::Comparator {
                        mode: ComparatorMode::Compare,
                        ..
                    })
                )
        }
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

    fn fire_route(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
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

    #[inline(always)]
    fn deliver_route(&mut self, route: RouteId, captured: Option<&[Option<ObservedSite>]>) -> Result<(), GraphExecutionError> {
        let result = self.deliver_route_inner(route, captured);
        result
    }

    fn deliver_route_inner(&mut self, route: RouteId, captured: Option<&[Option<ObservedSite>]>) -> Result<(), GraphExecutionError> {
        if let Some(binding) = self.program.instant_route_binding(route) {
            return self.deliver_instant_route(route, binding, captured);
        }
        let delivery_count = self.program.routes()[route.index()].deliveries.len();
        let classic_range = self.program.classic().route_ranges.get(route.index()).cloned().unwrap_or(0..0);
        let mut classic_cursor = classic_range.start;
        for index in 0..delivery_count {
            if classic_cursor < classic_range.end {
                let binding = self.program.classic().route_entries[classic_cursor];
                if binding.ordinal as usize == index {
                    classic_cursor += 1;
                    if self.run_classic_entry(binding.entry as usize) { continue; }
                }
            }
            let delivery = self.program.routes()[route.index()].deliveries[index];
            self.deliver_callback(route, delivery, captured)?;
        }
        Ok(())
    }

    #[inline(always)]
    fn deliver_callback(
        &mut self,
        route: RouteId,
        delivery: RouteDelivery,
        captured: Option<&[Option<ObservedSite>]>,
    ) -> Result<(), GraphExecutionError> {
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
        self.counts.callbacks += 1;
        #[cfg(test)]
        self.record_route_event(route, delivery, receiver);
        #[cfg(not(test))]
        let _ = route;
        match receiver {
            CallbackReceiver::Component(node) => self.update_component(node, delivery.kind, observed)?,
            CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => self.sample_piston(piston)?,
            CallbackReceiver::Contact(_) => {}
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

    pub(crate) fn inspect(&self, position: BlockPos) {
        match self.positions.get(&position) {
            Some(&node) => tracing::debug!(
                "Graph node {node:?}: {:?} {:?}",
                self.program.nodes()[node.index()].kind,
                match &self.state.components[node.index()] {
                    ComponentState::Wire(_) => ComponentState::Wire(self.wire_state(node)),
                    state => state.clone(),
                }
            ),
            None => tracing::debug!("could not find graph node at {position:?}"),
        }
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
    pub(crate) fn set_test_recording(&mut self, enabled: bool) {
        self.test_recorder.test_recording = enabled;
        if !enabled {
            self.clear_test_traces();
        }
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
#[path = "graph/tests/isolated_wire.rs"]
mod isolated_wire_tests;

#[cfg(test)]
#[path = "graph/tests/instant_reset.rs"]
mod instant_reset_tests;
