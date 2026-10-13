//! Graph-native execution for rich piston circuits. The World is read only at
//! compilation; flush and reset project graph state back to it.

use super::TickScheduler;
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
use mchprs_blocks::blocks::{
    Block, ComparatorMode, RedstonePiston, RedstoneWire, RedstoneWireSide,
};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{PistonEvent, TickEntry, TickPriority};
use rustc_hash::{FxHashMap, FxHashSet};
#[cfg(test)]
use std::cell::Cell;
use std::collections::VecDeque;
use std::fmt;

#[cfg(test)]
macro_rules! graph_test_trace {
    ($backend:expr, $value:expr) => {
        if $backend.test_recording {
            $backend.trace.push($value);
        }
    };
}
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
    MissingBlockState {
        node: NodeId,
    },
    MissingComponentSite {
        node: NodeId,
    },
    MissingNoteBlockSite {
        node: NodeId,
        position: BlockPos,
    },
    IncompleteWireNeighborhood {
        node: NodeId,
        position: BlockPos,
    },
    TickOutsideScheduler {
        delay: u32,
    },
    InvalidZeroDelayPriority {
        priority: TickPriority,
    },
    UnsupportedTickTarget {
        position: BlockPos,
        block_type: u32,
    },
    UnknownTickTarget {
        position: BlockPos,
    },
    UnsupportedComponent {
        position: Option<BlockPos>,
        kind: &'static str,
    },
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
                write!(
                    f,
                    "component node {} has no original block state",
                    node.index()
                )
            }
            Self::MissingComponentSite { node } => {
                write!(
                    f,
                    "wire node {} has no compiled component site",
                    node.index()
                )
            }
            Self::MissingNoteBlockSite { node, position } => write!(
                f,
                "noteblock node {} at {position:?} is missing its compiled support or above site",
                node.index()
            ),
            Self::IncompleteWireNeighborhood { node, position } => write!(
                f,
                "wire node {} is missing compiled query site at {position:?}",
                node.index()
            ),
            Self::TickOutsideScheduler { delay } => write!(
                f,
                "imported tick delay {delay} exceeds the graph scheduler ring"
            ),
            Self::InvalidZeroDelayPriority { priority } => {
                write!(f, "zero-delay graph tick has invalid priority {priority:?}")
            }
            Self::UnsupportedTickTarget {
                position,
                block_type,
            } => write!(
                f,
                "graph cannot dispatch tick for uncompiled block type {block_type} at {position:?}"
            ),
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

#[derive(Debug, Clone, Copy)]
struct PistonRequest {
    piston: PistonId,
    action: mchprs_world::PistonAction,
}

#[derive(Debug, Clone, Copy)]
struct DeferredPistonEvent {
    piston: PistonId,
    revision: u64,
    resource: Option<DeferredResourceEvent>,
    due_tick: u64,
    extending: bool,
}

#[derive(Debug, Clone, Copy)]
struct DeferredResourceEvent {
    resource: ResourceId,
    revision: u64,
    contact: ContactId,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum GraphObservedOperation {
    Callback {
        target: BlockPos,
        direction: Option<BlockFace>,
    },
    Sample {
        actor: BlockPos,
        extended: bool,
        powered: bool,
    },
    Request {
        actor: BlockPos,
        action: mchprs_world::PistonAction,
    },
    AcceptanceAttempt {
        actor: BlockPos,
        action: mchprs_world::PistonAction,
    },
    Accepted {
        actor: BlockPos,
        action: mchprs_world::PistonAction,
    },
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
pub(crate) enum GraphExecutionError {
    UnsupportedComponent {
        position: Option<BlockPos>,
        kind: &'static str,
    },
    CommandHostUnavailable {
        position: BlockPos,
    },
}

impl fmt::Display for GraphExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedComponent { position, kind } => {
                write!(f, "graph execution does not support {kind} at {position:?}")
            }
            Self::CommandHostUnavailable { position } => write!(
                f,
                "graph command block at {position:?} needs a host before its scheduled work can run"
            ),
        }
    }
}

impl std::error::Error for GraphExecutionError {}

trait GraphCommandHost {
    fn run_command(&mut self, command: &str, source: &str) -> Result<(), String>;
    fn publish_command_entity(&mut self, position: BlockPos, entity: BlockEntity);
}

impl<W: World> GraphCommandHost for W {
    fn run_command(&mut self, command: &str, source: &str) -> Result<(), String> {
        World::execute_command_block(self, command, source)
    }

    fn publish_command_entity(&mut self, position: BlockPos, entity: BlockEntity) {
        World::set_block_entity(self, position, entity);
    }
}

#[derive(Debug, Clone)]
struct ResolvedSite {
    profile: Option<ProfileId>,
    receiver: Option<CallbackReceiver>,
    block: Block,
    properties: ObservedProperties,
    solid: bool,
    transparent: bool,
    cube: bool,
}

impl ResolvedSite {
    fn observed(self) -> ObservedSite {
        ObservedSite {
            profile: self.profile,
            receiver: self.receiver,
            properties: self.properties,
        }
    }
}

pub(crate) struct GraphBackend {
    program: CircuitProgram,
    state: CircuitState,
    positions: FxHashMap<BlockPos, NodeId>,
    component_sites: Vec<Option<SiteId>>,
    scheduler: TickScheduler<ScheduledNodeTick>,
    pending: FxHashMap<PendingNodeTick, usize>,
    piston_requests: VecDeque<PistonRequest>,
    deferred_pistons: VecDeque<DeferredPistonEvent>,
    clock: Clock,
    dirty: Vec<bool>,
    effects: Vec<PresentationEffect>,
    /// This stops later graph work after an error may have left a partial tick.
    failure: Option<GraphExecutionError>,
    /// Reported work only; simulation never reads it.
    counts: GraphExecutionCounts,
    #[cfg(test)]
    trace: Vec<String>,
    #[cfg(test)]
    /// This disables detailed test traces during long episodes before formatting.
    test_recording: bool,
    #[cfg(test)]
    callback_order: Vec<(BlockPos, DeliveryKind)>,
    #[cfg(test)]
    comparator_updates: Vec<BlockPos>,
    #[cfg(test)]
    power_links_examined: Cell<usize>,
    #[cfg(test)]
    observed_operations: Vec<GraphObservedOperation>,
}

impl GraphBackend {
    #[cfg(test)]
    pub(crate) fn new(
        program: CircuitProgram,
        state: CircuitState,
        logical_tick: u64,
    ) -> Result<Self, GraphBackendError> {
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
        let limits = AnalysisLimits::for_budget(monitor.budget_multiplier());
        let mut prepared = passes::rich::prepare(world, bounds, ticks, &monitor, limits)
            .map_err(map_preparation_error)?;
        let options = crate::redpiler::CompilerOptions::default();
        let pipeline = passes::default_graph_pipeline(&options);
        passes::run_graph_pipeline(
            &mut prepared.graph,
            &options,
            &prepared.entry,
            &pipeline,
            &monitor,
        )
        .map_err(GraphBackendError::PassPipeline)?;
        prepared.finish_pipeline_statistics(&monitor);
        let lowered = prepared.lower(&monitor).map_err(GraphBackendError::Graph)?;
        Self::from_lowered(lowered, &monitor)
    }

    pub(crate) fn from_lowered(
        lowered: LoweredCircuit,
        monitor: &TaskMonitor,
    ) -> Result<Self, GraphBackendError> {
        Self::validate_supported(&lowered.program)?;
        let mut backend = Self::new_with_positions(
            lowered.program,
            lowered.state,
            lowered.logical_tick,
            lowered.positions,
        )?;
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
                (
                    position.offset(facing.opposite()),
                    false,
                    block.property("conditional") == Some("true"),
                ),
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
                        SiteSource::Component(component) => {
                            program.nodes()[component.index()].block
                        }
                        SiteSource::Contact(contact) => program.contacts()[contact.index()]
                            .variants
                            .iter()
                            .filter_map(|variant| program.profile_block(variant.profile))
                            .find(|block| block.is_command_block()),
                    })
                    .is_some_and(|neighbor_block| {
                        neighbor_block.is_command_block()
                            && (!only_chain || neighbor_block.get_name() == "chain_command_block")
                            && !program.command_blocks().iter().any(|plan| {
                                program.nodes()[plan.node.index()].position == Some(neighbor)
                            })
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
            if matches!(
                node.kind,
                CircuitNodeKind::Electrical(NodeType::NoteBlock { .. })
            ) && !program
                .note_blocks()
                .iter()
                .any(|plan| plan.node.index() == index)
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
                matches!(
                    program.profile_block(profile),
                    Some(Block::RedstoneWire { .. })
                )
                .then_some(SiteId::new(index as u32))
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
            if external_wires.contains(&link.from)
                && matches!(
                    link.from_port,
                    PowerPort::Weak(_) | PowerPort::Strong(_) | PowerPort::Stored
                )
            {
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
                    pending.extend(
                        conducted_users
                            .get(&target)
                            .into_iter()
                            .flatten()
                            .map(|user| (*user, wire)),
                    );
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
                CircuitNodeKind::Electrical(_)
                    | CircuitNodeKind::Observer { .. }
                    | CircuitNodeKind::Wire(_)
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
            let position = program.nodes()[wire.node.index()]
                .position
                .expect("wire nodes have positions");
            if program.sites()[root_site.index()].position != position {
                return Err(GraphBackendError::MissingComponentSite { node: wire.node });
            }
            for (site, expected) in wire
                .canonical_neighbors
                .iter()
                .zip(crate::world::wire_cache::positions(position))
            {
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
        Ok(Self {
            program,
            state,
            positions,
            component_sites,
            scheduler: TickScheduler::default(),
            pending: FxHashMap::default(),
            piston_requests: VecDeque::new(),
            deferred_pistons: VecDeque::new(),
            clock: Clock {
                logical_tick,
                phase: GraphPhase::BetweenTicks,
                scheduled_advanced: false,
            },
            dirty: vec![false; node_count],
            effects: Vec::new(),
            failure: None,
            counts: GraphExecutionCounts::default(),
            #[cfg(test)]
            trace: Vec::new(),
            #[cfg(test)]
            test_recording: false,
            #[cfg(test)]
            callback_order: Vec::new(),
            #[cfg(test)]
            comparator_updates: Vec::new(),
            #[cfg(test)]
            power_links_examined: Cell::new(0),
            #[cfg(test)]
            observed_operations: Vec::new(),
        })
    }

    fn import_ticks(&mut self, ticks: Vec<ImportedTick>) -> Result<(), GraphBackendError> {
        for tick in ticks {
            if tick.delay as usize >= SCHEDULER_BUCKETS {
                return Err(GraphBackendError::TickOutsideScheduler { delay: tick.delay });
            }
            if tick.delay == 0 && tick.priority != TickPriority::NanoTick {
                return Err(GraphBackendError::InvalidZeroDelayPriority {
                    priority: tick.priority,
                });
            }
            let current_registry_type =
                tick.current_registry_type
                    .ok_or(GraphBackendError::UnknownTickTarget {
                        position: tick.position,
                    })?;
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
            if node.is_none()
                && registry_type == current_registry_type
                && current_registry_type != Block::Air.registry_id()
            {
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
            self.scheduler
                .schedule_half_tick(scheduled, tick.delay as usize, tick.priority);
            *self.pending.entry(scheduled.into()).or_default() += 1;
        }
        Ok(())
    }

    fn registry_type(&self, node: NodeId) -> u32 {
        self.program.nodes()[node.index()]
            .block
            .unwrap()
            .registry_id()
    }

    fn schedule(&mut self, node: NodeId, delay: usize, priority: TickPriority) {
        self.schedule_with(node, delay.saturating_mul(2), priority, true);
    }

    fn schedule_half_tick(&mut self, node: NodeId, delay: usize, priority: TickPriority) {
        self.schedule_with(node, delay, priority, false);
    }

    fn schedule_with(
        &mut self,
        node: NodeId,
        delay: usize,
        priority: TickPriority,
        _game_tick_delay: bool,
    ) {
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
        if self.pending.contains_key(&key) {
            return;
        }
        self.scheduler.schedule_half_tick(tick, delay, priority);
        self.pending.insert(key, 1);
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "schedule {:?} +{} {} {priority:?}",
                node,
                if _game_tick_delay { delay / 2 } else { delay },
                if _game_tick_delay {
                    "ticks"
                } else {
                    "half-ticks"
                }
            )
        );
    }

    fn is_pending(&self, node: NodeId) -> bool {
        self.pending.contains_key(&PendingNodeTick {
            node: Some(node),
            position: self.position(node),
            block_type: Some(self.registry_type(node)),
            registry_type: self.registry_type(node),
            current_registry_type: self.registry_type(node),
        })
    }

    fn popped(&mut self, tick: ScheduledNodeTick) {
        let key = tick.into();
        if let Some(count) = self.pending.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                self.pending.remove(&key);
            }
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), GraphExecutionError> {
        self.tick_with_host(None)
    }

    pub(crate) fn tick_with_world<W: World>(
        &mut self,
        world: &mut W,
    ) -> Result<(), GraphExecutionError> {
        let result = self.tick_with_host(Some(world));
        world.piston_state_mut().logical_tick = self.clock.logical_tick;
        result
    }

    fn tick_with_host<'h>(
        &mut self,
        mut host: Option<&'h mut (dyn GraphCommandHost + 'h)>,
    ) -> Result<(), GraphExecutionError> {
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
        self.scheduler
            .iter()
            .find(|(tick, delay, _)| {
                *delay <= 1
                    && tick.registry_type == tick.current_registry_type
                    && tick.node.is_some_and(|node| {
                        matches!(
                            self.program.nodes()[node.index()].kind,
                            CircuitNodeKind::Electrical(NodeType::CommandBlock {
                                chain: false,
                                ..
                            })
                        )
                    })
            })
            .map(|(tick, _, _)| tick.position)
    }

    fn command_host_preflight_position(&self) -> Option<BlockPos> {
        self.due_command_position().or_else(|| {
            if !self.scheduler.iter().any(|(_, delay, _)| delay <= 1) {
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

    fn tick_phases<'h>(
        &mut self,
        host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>,
    ) -> Result<(), GraphExecutionError> {
        self.counts.ticks += 1;
        self.clock.logical_tick += 1;
        self.clock.phase = GraphPhase::ScheduledTicks;
        self.clock.scheduled_advanced = false;
        #[cfg(test)]
        graph_test_trace!(self, "phase scheduled_ticks".into());
        if let Err(error) = self.drain_current_ticks(host) {
            self.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }

        self.scheduler.end_last_tick_move_next();
        self.clock.scheduled_advanced = true;
        #[cfg(test)]
        graph_test_trace!(self, "clock.advance".into());
        #[cfg(test)]
        graph_test_trace!(self, "phase scheduled_ticks_after_advance".into());
        if let Err(error) = self.drain_current_ticks(host) {
            self.clock.phase = GraphPhase::BetweenTicks;
            self.clock.scheduled_advanced = false;
            return Err(error);
        }

        self.clock.phase = GraphPhase::PistonActions;
        #[cfg(test)]
        graph_test_trace!(self, "phase piston_actions".into());
        if let Err(error) = self.drain_piston_actions() {
            self.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }
        self.clock.phase = GraphPhase::DeferredConnections;
        #[cfg(test)]
        graph_test_trace!(self, "phase deferred_connections".into());
        if let Err(error) = self.publish_deferred_pistons() {
            self.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }
        self.clock.phase = GraphPhase::BetweenTicks;
        self.clock.scheduled_advanced = false;
        #[cfg(test)]
        graph_test_trace!(self, "phase between_ticks".into());
        Ok(())
    }

    fn drain_current_ticks<'h>(
        &mut self,
        host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>,
    ) -> Result<(), GraphExecutionError> {
        while let Some(tick) = self.scheduler.this_tick().pop_first() {
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

    pub(crate) fn set_pressure_plate(
        &mut self,
        position: BlockPos,
        powered: bool,
    ) -> Result<(), GraphExecutionError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.set_pressure_plate_inner(position, powered);
        self.latch_error(&result);
        result
    }

    fn set_pressure_plate_inner(
        &mut self,
        position: BlockPos,
        powered: bool,
    ) -> Result<(), GraphExecutionError> {
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

    fn tick_component<'h>(
        &mut self,
        node: NodeId,
        host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>,
    ) -> Result<(), GraphExecutionError> {
        match self.program.nodes()[node.index()].kind.clone() {
            CircuitNodeKind::Wire(_) => Ok(()),
            CircuitNodeKind::Observer { .. } => self.tick_observer(node),
            CircuitNodeKind::Electrical(NodeType::Torch) => self.tick_torch(node),
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, .. }) => {
                self.tick_repeater(node, delay)
            }
            CircuitNodeKind::Electrical(NodeType::Comparator {
                mode, facing_diode, ..
            }) => self.tick_comparator(node, mode, facing_diode),
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
            CircuitNodeKind::Electrical(NodeType::CopperBulb) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Constant) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Observer { .. } | NodeType::Wire) => {
                Err(self.unsupported(node, "invalid generic observer or wire"))
            }
            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => {
                self.tick_command_block(node, host)
            }
            CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => Ok(()),
            CircuitNodeKind::Electrical(NodeType::Trapdoor) => {
                Err(self.unsupported(node, "trapdoor"))
            }
            CircuitNodeKind::Piston(piston) => self.sample_piston(piston),
            CircuitNodeKind::Contact(_) => Ok(()),
        }
    }

    fn update_component(
        &mut self,
        node: NodeId,
        kind: DeliveryKind,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
        let direction = match kind {
            DeliveryKind::Update { direction } => direction,
            DeliveryKind::Shape { direction } => {
                return if matches!(
                    self.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Wire(_)
                ) {
                    self.update_wire_shape(node, direction, captured)
                } else {
                    Ok(())
                };
            }
        };
        #[cfg(test)]
        if self.test_recording {
            self.callback_order.push((self.position(node), kind));
        }
        match self.program.nodes()[node.index()].kind.clone() {
            CircuitNodeKind::Observer { .. } => self.update_observer(node, direction, captured),
            CircuitNodeKind::Electrical(NodeType::Torch) => self.update_torch(node, captured),
            CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => {
                self.update_repeater(node, captured)
            }
            CircuitNodeKind::Electrical(NodeType::Comparator { .. }) => {
                self.update_comparator(node)
            }
            CircuitNodeKind::Electrical(NodeType::Lamp) => self.update_lamp(node, captured),
            CircuitNodeKind::Electrical(NodeType::CopperBulb) => self.update_bulb(node),
            CircuitNodeKind::Wire(_) => self.evaluate_seed(node, captured),
            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => {
                self.update_command_power(node)
            }
            CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => self.update_note(node),
            CircuitNodeKind::Electrical(NodeType::Trapdoor) => {
                Err(self.unsupported(node, "trapdoor"))
            }
            CircuitNodeKind::Piston(piston) => self.sample_piston(piston),
            CircuitNodeKind::Contact(_) => Ok(()),
            CircuitNodeKind::Electrical(
                NodeType::Lever | NodeType::Button | NodeType::PressurePlate,
            ) => Ok(()),
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
                repeating,
                chain,
                automatic,
                ..
            }) => (repeating, chain, automatic),
            _ => return Err(self.unsupported(node, "invalid command block binding")),
        };
        let Some(entity) = self.node_state(node).command_entity.as_ref() else {
            return Err(self.unsupported(node, "command block state"));
        };
        let entity_powered = entity.powered;
        let last_execution = entity.last_execution;
        let rising = powered && !entity_powered;
        let changed = entity_powered != powered;
        let should_start = rising || (automatic && last_execution < 0);
        let should_repeat = repeating && (powered || automatic);
        let should_schedule = !chain && (should_start || should_repeat) && !self.is_pending(node);
        let condition_met = should_schedule.then(|| self.command_condition_met(node));
        let position = self.position(node);
        let Some(ComponentState::Electrical(state)) = self.state.components.get_mut(node.index())
        else {
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
        if should_schedule || changed {
            self.dirty[node.index()] = true;
        }
        if should_schedule {
            self.schedule_half_tick(node, 1, TickPriority::Normal);
        }
        Ok(())
    }

    fn tick_command_block<'h>(
        &mut self,
        node: NodeId,
        host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>,
    ) -> Result<(), GraphExecutionError> {
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
        let mut may_continue = self
            .node_state(node)
            .command_entity
            .as_ref()
            .is_some_and(|entity| entity.condition_met);
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
            CircuitNodeKind::Electrical(NodeType::CommandBlock {
                repeating: true,
                ..
            })
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

    fn execute_command<'h>(
        &mut self,
        node: NodeId,
        host: &mut Option<&'h mut (dyn GraphCommandHost + 'h)>,
    ) -> Result<bool, GraphExecutionError> {
        let position = self.position(node);
        if host.is_none() {
            return Err(GraphExecutionError::CommandHostUnavailable { position });
        }
        let Some(mut entity) = self.node_state(node).command_entity.clone() else {
            return Err(self.unsupported(node, "command block entity state"));
        };
        let tick = self.clock.logical_tick.min(i64::MAX as u64) as i64;
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
                        entity.last_output = Some(
                            serde_json::json!({"text":crate::messages::COMMAND_BLOCK_EXECUTED})
                                .to_string(),
                        );
                    }
                }
                Err(error) => {
                    if entity.track_output {
                        entity.last_output =
                            Some(serde_json::json!({"text":error,"color":"red"}).to_string());
                    }
                }
            }
        }
        if entity.update_last_execution {
            entity.last_execution = tick;
        }
        let success_count = entity.success_count.clamp(0, 15) as u8;
        let Some(ComponentState::Electrical(state)) = self.state.components.get_mut(node.index())
        else {
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
        self.fire_route_unconditionally(node)?;
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
        let unblocked = self
            .resolve_site(plan.above, None)
            .is_some_and(|site| site.block == Block::Air);
        let CircuitNodeKind::Electrical(NodeType::NoteBlock { note, .. }) =
            &self.program.nodes()[node.index()].kind
        else {
            return Err(self.unsupported(node, "invalid noteblock binding"));
        };
        let Some(ComponentState::Electrical(state)) = self.state.components.get_mut(node.index())
        else {
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

    fn sample_piston(&mut self, piston: PistonId) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let powered = self.input_strength(spec.node, InputPort::Main) > 0
            || self.input_strength(spec.node, InputPort::Quasi) > 0;
        let state = self.state.pistons[piston.index()];
        if state.availability.base_pending() {
            return Ok(());
        }
        self.counts.piston_samples += 1;
        #[cfg(test)]
        if self.test_recording {
            self.observed_operations
                .push(GraphObservedOperation::Sample {
                    actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                    extended: state.extended,
                    powered,
                });
        }
        let action = match (powered, state.extended) {
            (true, false) if self.piston_lane_available(piston) => {
                mchprs_world::PistonAction::Extend
            }
            (false, true) => {
                let early_resource_retraction = match spec.lane {
                    PistonLane::SingleResource { far } => self
                        .piston_resource_at(piston, far)
                        .and_then(|resource| {
                            self.pending_resource_extension(resource, far, spec.facing)
                        })
                        .is_some_and(|event| {
                            event.due_tick > self.clock.logical_tick
                                || matches!(
                                    self.clock.phase,
                                    GraphPhase::ScheduledTicks | GraphPhase::PistonActions
                                )
                        }),
                    PistonLane::Empty => false,
                };
                if early_resource_retraction {
                    mchprs_world::PistonAction::RetractWithoutPull
                } else {
                    mchprs_world::PistonAction::Retract
                }
            }
            _ => return Ok(()),
        };
        self.enqueue_piston(PistonRequest { piston, action });
        Ok(())
    }

    fn enqueue_piston(&mut self, request: PistonRequest) {
        if !self
            .piston_requests
            .iter()
            .any(|queued| queued.piston == request.piston && queued.action == request.action)
        {
            #[cfg(test)]
            if self.test_recording {
                self.observed_operations
                    .push(GraphObservedOperation::Request {
                        actor: self.program.nodes()
                            [self.program.pistons()[request.piston.index()].node.index()]
                        .position
                        .unwrap(),
                        action: request.action,
                    });
            }
            #[cfg(test)]
            graph_test_trace!(
                self,
                format!("piston request {:?} {:?}", request.piston, request.action)
            );
            self.piston_requests.push_back(request);
        }
    }

    fn drain_piston_actions(&mut self) -> Result<(), GraphExecutionError> {
        while let Some(request) = self.piston_requests.pop_front() {
            self.accept_action(request)?;
        }
        Ok(())
    }

    fn accept_action(&mut self, request: PistonRequest) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[request.piston.index()];
        let state = self.state.pistons[request.piston.index()];
        #[cfg(test)]
        if self.test_recording {
            self.observed_operations
                .push(GraphObservedOperation::AcceptanceAttempt {
                    actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                    action: request.action,
                });
        }
        let powered = self.input_strength(spec.node, InputPort::Main) > 0
            || self.input_strength(spec.node, InputPort::Quasi) > 0;
        match request.action {
            mchprs_world::PistonAction::Extend
                if powered
                    && !state.extended
                    && state.availability == PistonAvailability::Ready
                    && self.piston_lane_available(request.piston) =>
            {
                #[cfg(test)]
                graph_test_trace!(
                    self,
                    format!("piston accept {:?} {:?}", request.piston, request.action)
                );
                self.extend_connections(request.piston)?;
                self.accepted(request, state.extended);
                #[cfg(test)]
                if self.test_recording {
                    self.observed_operations
                        .push(GraphObservedOperation::Accepted {
                            actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                            action: request.action,
                        });
                }
            }
            mchprs_world::PistonAction::Retract
            | mchprs_world::PistonAction::RetractWithoutPull
                if !powered
                    && state.extended
                    && !state.availability.base_pending()
                    && self.piston_retraction_available(
                        request.piston,
                        request.action == mchprs_world::PistonAction::RetractWithoutPull,
                    ) =>
            {
                #[cfg(test)]
                graph_test_trace!(
                    self,
                    format!("piston accept {:?} {:?}", request.piston, request.action)
                );
                self.retract_connections(
                    request.piston,
                    request.action == mchprs_world::PistonAction::RetractWithoutPull,
                )?;
                self.accepted(request, state.extended);
                #[cfg(test)]
                if self.test_recording {
                    self.observed_operations
                        .push(GraphObservedOperation::Accepted {
                            actor: self.program.nodes()[spec.node.index()].position.unwrap(),
                            action: request.action,
                        });
                }
            }
            _ => {
                #[cfg(test)]
                graph_test_trace!(
                    self,
                    format!("piston cancelled {:?} {:?}", request.piston, request.action)
                );
            }
        }
        Ok(())
    }

    fn accepted(&mut self, request: PistonRequest, extended: bool) {
        self.counts.piston_actions += 1;
        let spec = self.program.pistons()[request.piston.index()];
        self.effects.push(PresentationEffect::BlockAction {
            position: self.position(spec.node),
            action: BlockAction::Piston {
                action: request.action,
                piston: RedstonePiston {
                    facing: spec.facing.into(),
                    sticky: spec.sticky,
                    extended,
                },
            },
        });
    }

    fn set_resource_contact(
        &mut self,
        resource: ResourceId,
        contact: ContactId,
        enabled: bool,
    ) -> Option<(ResourceId, u64)> {
        let index = resource.index();
        debug_assert!(self.program.resources()[index].contacts.contains(&contact));
        debug_assert!(
            self.program
                .resources_at_contact(contact)
                .iter()
                .all(|other| {
                    other.index() == index || self.state.resources[other.index()].at != contact
                })
        );
        let state = &mut self.state.resources[index];
        if state.at == contact && state.enabled == enabled {
            return None;
        }
        state.at = contact;
        state.enabled = enabled;
        state.revision = state.revision.wrapping_add(1);
        Some((ResourceId::new(index as u32), state.revision))
    }

    fn resource_for_lane(&self, piston: PistonId) -> Option<ResourceId> {
        self.program.resource_for_piston(piston)
    }

    fn piston_resource_at(&self, piston: PistonId, contact: ContactId) -> Option<ResourceId> {
        let resource = self.resource_for_lane(piston)?;
        (self.state.resources[resource.index()].at == contact).then_some(resource)
    }

    fn pending_resource_extension(
        &self,
        resource_id: ResourceId,
        contact: ContactId,
        facing: BlockFace,
    ) -> Option<&DeferredPistonEvent> {
        let state = self.state.resources[resource_id.index()];
        if state.enabled || state.at != contact {
            return None;
        }
        self.deferred_pistons.iter().find(|event| {
            event.extending
                && self.program.pistons()[event.piston.index()].facing == facing
                && event.resource.is_some_and(|resource| {
                    resource.resource == resource_id
                        && resource.revision == state.revision
                        && resource.contact == contact
                })
        })
    }

    fn resource_contact_available(
        &self,
        resource: ResourceId,
        contact: ContactId,
        vacated_head: Option<PistonId>,
    ) -> bool {
        let Some(spec) = self.program.resources().get(resource.index()) else {
            return false;
        };
        let Some(state) = self.state.resources.get(resource.index()).copied() else {
            return false;
        };
        if !spec.contacts.contains(&contact)
            || self
                .program
                .resources_at_contact(contact)
                .iter()
                .any(|other| {
                    other.index() != resource.index()
                        && self.state.resources[other.index()].at == contact
                })
        {
            return false;
        }
        for &id in self.program.base_pistons_at_contact(contact) {
            if !self.state.pistons[id.index()].availability.base_pending() {
                return false;
            }
        }
        for &id in self.program.head_pistons_at_contact(contact) {
            let state = self.state.pistons[id.index()];
            let head_is_present = state.head_present
                && matches!(
                    state.availability,
                    PistonAvailability::Ready
                        | PistonAvailability::HeadPending
                        | PistonAvailability::BasePendingHeadPresent
                );
            if head_is_present && vacated_head != Some(id) {
                return false;
            }
        }
        if state.at == contact && state.enabled {
            return true;
        }
        if vacated_head.is_some_and(|id| self.program.pistons()[id.index()].near == contact) {
            return true;
        }
        self.contact_block(contact) == Block::Air
    }

    fn piston_retraction_available(&self, piston: PistonId, without_pull: bool) -> bool {
        let spec = self.program.pistons()[piston.index()];
        let PistonLane::SingleResource { far } = spec.lane else {
            return true;
        };
        let Some(resource) = self.piston_resource_at(piston, far) else {
            return true;
        };
        let state = self.state.resources[resource.index()];
        let matching_extension = self
            .pending_resource_extension(resource, far, spec.facing)
            .is_some();
        if !state.enabled {
            !matching_extension || self.resource_contact_available(resource, far, None)
        } else {
            without_pull || self.resource_contact_available(resource, spec.near, Some(piston))
        }
    }

    fn contact_block(&self, contact: ContactId) -> Block {
        let variant = self.program.contacts()[contact.index()]
            .variants
            .iter()
            .find(|variant| self.guard_matches(variant.when, None))
            .expect("compiled contact has a selected variant");
        self.program
            .profile_block(variant.profile)
            .expect("compiled contact profile was validated")
    }

    fn piston_lane_available(&self, piston: PistonId) -> bool {
        let spec = self.program.pistons()[piston.index()];
        let near = self.contact_block(spec.near);
        if ContainerType::from_block(near).is_some()
            || matches!(near, Block::MovingPiston { .. } | Block::PistonHead { .. })
        {
            return false;
        }
        let Some(resource) = self.piston_resource_at(piston, spec.near) else {
            return near == Block::Air && self.resource_at_contact(spec.near).is_none();
        };
        let state = self.state.resources[resource.index()];
        let PistonLane::SingleResource { far } = spec.lane else {
            return false;
        };
        state.enabled
            && near
                == self
                    .program
                    .profile_block(self.program.resources()[resource.index()].material)
                    .unwrap()
            && self.resource_contact_available(resource, far, None)
    }

    fn resource_at_contact(&self, contact: ContactId) -> Option<ResourceId> {
        self.program
            .resources_at_contact(contact)
            .iter()
            .copied()
            .find(|resource| self.state.resources[resource.index()].at == contact)
    }

    fn extend_connections(&mut self, piston: PistonId) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let resource_to_move = self.piston_resource_at(piston, spec.near);
        if let (Some(resource), PistonLane::SingleResource { far }) = (resource_to_move, spec.lane)
        {
            debug_assert!(self.resource_contact_available(resource, far, None));
        }
        let revision = {
            let state = &mut self.state.pistons[piston.index()];
            state.revision = state.revision.wrapping_add(1);
            state.availability = PistonAvailability::HeadPending;
            state.head_present = true;
            state.revision
        };
        let resource = match (resource_to_move, spec.lane) {
            (Some(resource), PistonLane::SingleResource { far }) => {
                self.set_resource_contact(resource, far, false)
            }
            _ => None,
        };
        let deferred_resource = resource.map(|(resource, revision)| DeferredResourceEvent {
            resource,
            revision,
            contact: match spec.lane {
                PistonLane::SingleResource { far } => far,
                PistonLane::Empty => unreachable!(),
            },
        });
        // Nested callbacks must see the transfer's direction and deadline.
        self.deferred_pistons.push_back(DeferredPistonEvent {
            piston,
            revision,
            resource: deferred_resource,
            due_tick: self.clock.logical_tick + 2,
            extending: true,
        });
        if resource.is_some()
            && let Some(route) = spec.routes.far_shape_changed
        {
            self.deliver_route(route, None)?;
        }
        self.deliver_route(spec.routes.near_changed, None)?;
        if resource.is_some() {
            self.deliver_route(spec.routes.near_changed, None)?;
        }
        self.state.pistons[piston.index()].extended = true;
        self.deliver_route(spec.routes.base_changed, None)?;
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!("piston extend {:?} revision={revision}", piston)
        );
        Ok(())
    }

    fn retract_connections(
        &mut self,
        piston: PistonId,
        without_pull: bool,
    ) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let resource_at_far = match spec.lane {
            PistonLane::SingleResource { far } => self.piston_resource_at(piston, far),
            PistonLane::Empty => None,
        };
        let matching_extension = match (resource_at_far, spec.lane) {
            (Some(resource), PistonLane::SingleResource { far }) => self
                .pending_resource_extension(resource, far, spec.facing)
                .is_some(),
            _ => false,
        };
        let resource_enabled =
            resource_at_far.is_some_and(|resource| self.state.resources[resource.index()].enabled);
        let restore_far = matching_extension;
        let pull_resource = !without_pull && resource_enabled;
        if let (Some(resource), PistonLane::SingleResource { far }) = (resource_at_far, spec.lane) {
            debug_assert!(if restore_far {
                self.resource_contact_available(resource, far, None)
            } else if pull_resource {
                self.resource_contact_available(resource, spec.near, Some(piston))
            } else {
                true
            });
        }
        let interrupted_head =
            self.state.pistons[piston.index()].availability == PistonAvailability::HeadPending;
        let revision = {
            let state = &mut self.state.pistons[piston.index()];
            state.revision = state.revision.wrapping_add(1);
            state.availability = if interrupted_head {
                PistonAvailability::PublishingHeadRemoval
            } else {
                PistonAvailability::BasePendingHeadPresent
            };
            state.revision
        };
        if interrupted_head {
            self.deliver_route(spec.routes.near_changed, None)?;
            self.state.pistons[piston.index()].head_present = false;
            self.state.pistons[piston.index()].availability = PistonAvailability::BasePending;
            self.deliver_route(spec.routes.base_changed, None)?;
        } else {
            self.deliver_route(spec.routes.base_changed, None)?;
            self.state.pistons[piston.index()].availability = PistonAvailability::BasePending;
            self.state.pistons[piston.index()].head_present = false;
        }
        let restored_far = if restore_far {
            let resource = resource_at_far.expect("restore requires a selected resource");
            let PistonLane::SingleResource { far } = spec.lane else {
                unreachable!();
            };
            self.set_resource_contact(resource, far, true)
        } else {
            None
        };
        if restored_far.is_some()
            && let Some(route) = spec.routes.resource_restored_far
        {
            self.deliver_route(route, None)?;
        }
        let resource = match (pull_resource, resource_at_far, spec.lane) {
            (true, Some(resource), PistonLane::SingleResource { .. }) => {
                self.set_resource_contact(resource, spec.near, false)
            }
            _ => None,
        };
        if !without_pull && resource.is_some() {
            if let Some(route) = spec.routes.far_changed {
                self.deliver_route(route, None)?;
            }
        }
        self.deliver_route(spec.routes.near_changed, None)?;
        let deferred_resource = resource.map(|(resource, revision)| DeferredResourceEvent {
            resource,
            revision,
            contact: spec.near,
        });
        self.deferred_pistons.push_back(DeferredPistonEvent {
            piston,
            revision,
            resource: deferred_resource,
            due_tick: self.clock.logical_tick + 2,
            extending: false,
        });
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!("piston retract {:?} revision={revision}", piston)
        );
        Ok(())
    }

    fn publish_deferred_pistons(&mut self) -> Result<(), GraphExecutionError> {
        let due = self
            .deferred_pistons
            .iter()
            .take_while(|event| event.due_tick <= self.clock.logical_tick)
            .count();
        for _ in 0..due {
            let event = self.deferred_pistons.pop_front().unwrap();
            self.apply_deferred_event(event)?;
        }
        Ok(())
    }

    fn apply_deferred_event(
        &mut self,
        event: DeferredPistonEvent,
    ) -> Result<(), GraphExecutionError> {
        let state = self.state.pistons[event.piston.index()];
        let spec = self.program.pistons()[event.piston.index()];
        let actor_valid = state.revision == event.revision;
        self.counts.deferred_publications += 1;

        if event.extending {
            self.apply_deferred_resource(event, spec)?;
            if actor_valid {
                self.state.pistons[event.piston.index()].availability = PistonAvailability::Ready;
                self.state.pistons[event.piston.index()].head_present = true;
                self.deliver_route(spec.routes.head_restored, None)?;
            } else {
                #[cfg(test)]
                graph_test_trace!(
                    self,
                    format!(
                        "piston stale {:?} revision={}",
                        event.piston, event.revision
                    )
                );
            }
        } else {
            if actor_valid {
                self.state.pistons[event.piston.index()].extended = false;
                self.state.pistons[event.piston.index()].head_present = false;
                self.state.pistons[event.piston.index()].availability = PistonAvailability::Ready;
                self.deliver_route(spec.routes.base_restored, None)?;
            } else {
                #[cfg(test)]
                graph_test_trace!(
                    self,
                    format!(
                        "piston stale {:?} revision={}",
                        event.piston, event.revision
                    )
                );
            }
            self.apply_deferred_resource(event, spec)?;
        }
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "piston restored {:?} revision={}",
                event.piston, event.revision
            )
        );
        Ok(())
    }

    fn apply_deferred_resource(
        &mut self,
        event: DeferredPistonEvent,
        spec: PistonSpec,
    ) -> Result<(), GraphExecutionError> {
        let Some(resource_event) = event.resource else {
            return Ok(());
        };
        let current = self.state.resources[resource_event.resource.index()];
        if current.revision != resource_event.revision
            || current.at != resource_event.contact
            || current.enabled
            || !self.resource_contact_available(
                resource_event.resource,
                resource_event.contact,
                None,
            )
        {
            #[cfg(test)]
            graph_test_trace!(
                self,
                format!(
                    "resource stale {:?} revision={}",
                    resource_event.resource, resource_event.revision
                )
            );
            return Ok(());
        }
        let (_, revision) = self
            .set_resource_contact(resource_event.resource, resource_event.contact, true)
            .expect("validated disabled resource must become enabled");
        debug_assert_eq!(revision, resource_event.revision.wrapping_add(1));
        let route = if event.extending {
            spec.routes.resource_restored_far
        } else {
            spec.routes.resource_restored_near
        };
        if let Some(route) = route {
            self.deliver_route(route, None)?;
        }
        Ok(())
    }

    fn wire_plan(&self, node: NodeId) -> &WirePlan {
        let CircuitNodeKind::Wire(index) = &self.program.nodes()[node.index()].kind else {
            unreachable!("wire handler received a non-wire node");
        };
        &self.program.wires()[*index]
    }

    fn wire_state(&self, node: NodeId) -> RedstoneWire {
        match &self.state.components[node.index()] {
            ComponentState::Wire(wire) => *wire,
            _ => unreachable!("wire node has non-wire state"),
        }
    }

    fn set_wire(&mut self, node: NodeId, wire: RedstoneWire) {
        let ComponentState::Wire(current) = &mut self.state.components[node.index()] else {
            unreachable!("wire node has non-wire state");
        };
        if *current == wire {
            return;
        }
        #[cfg(test)]
        let old = *current;
        *current = wire;
        self.dirty[node.index()] = true;
        #[cfg(test)]
        graph_test_trace!(self, format!("wire state {node:?} {old:?} -> {wire:?}"));
    }

    fn update_wire_shape(
        &mut self,
        node: NodeId,
        direction: BlockFace,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
        let wire = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Wire(wire) => Some(wire),
                _ => None,
            })
            .unwrap_or_else(|| self.wire_state(node));
        let plan = self.wire_plan(node).clone();
        let raw = self.raw_wire_sides(&plan, wire);
        let updated = crate::redstone::wire::on_neighbor_changed_from(wire, raw, direction);
        if updated != wire {
            self.set_wire(node, updated);
            if let Some(route) = plan.shape_route {
                self.deliver_route(route, None)?;
            }
        }
        Ok(())
    }

    fn evaluate_seed(
        &mut self,
        node: NodeId,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
        let mut wire = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Wire(wire) => Some(wire),
                _ => None,
            })
            .unwrap_or_else(|| self.wire_state(node));
        let plan = self.wire_plan(node).clone();
        #[cfg(test)]
        let (power, block_power, wire_power) = self.seed_power(&plan);
        #[cfg(not(test))]
        let (power, _, _) = self.seed_power(&plan);
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire seed {node:?} operands=live-sites/live-strong block={block_power} wire={wire_power} power {} -> {power}",
                wire.power,
            )
        );
        if wire.power == power {
            return Ok(());
        }
        wire.power = power;
        self.set_wire(node, wire);
        self.counts.wire_walks += 1;
        self.evaluate_turbo_wire(node)
    }

    fn seed_power(&self, plan: &WirePlan) -> (u8, u8, u8) {
        let up_solid = self.live_site(plan.canonical_neighbors[3]).solid;
        let mut block_power = 0;
        let mut wire_power = 0;
        for face in BlockFace::values() {
            let (index, above, below) = match face {
                BlockFace::Top => (3, None, None),
                BlockFace::Bottom => (2, None, None),
                BlockFace::North => (4, Some(20), Some(17)),
                BlockFace::South => (5, Some(21), Some(18)),
                BlockFace::East => (1, Some(13), Some(12)),
                BlockFace::West => (0, Some(8), Some(7)),
            };
            let site = plan.canonical_neighbors[index];
            let neighbor = self.live_site(site);
            block_power = block_power.max(self.power_no_dust(site, face, None));
            if let Block::RedstoneWire { wire } = neighbor.block {
                wire_power = wire_power.max(wire.power);
            }
            if let (Some(above), Some(below)) = (above, below) {
                if !up_solid && !neighbor.transparent {
                    if let Block::RedstoneWire { wire } =
                        self.site_block(plan.canonical_neighbors[above])
                    {
                        wire_power = wire_power.max(wire.power);
                    }
                }
                if !neighbor.solid {
                    if let Block::RedstoneWire { wire } =
                        self.site_block(plan.canonical_neighbors[below])
                    {
                        wire_power = wire_power.max(wire.power);
                    }
                }
            }
        }
        (
            block_power.max(wire_power.saturating_sub(1)),
            block_power,
            wire_power,
        )
    }

    fn site_block(&self, site: SiteId) -> Block {
        self.live_site(site).block
    }

    fn live_site(&self, site: SiteId) -> ResolvedSite {
        self.resolve_site(site, None)
            .expect("wire query site was validated during preparation")
    }

    fn raw_wire_sides(&self, plan: &WirePlan, wire: RedstoneWire) -> RedstoneWire {
        let neighbors = plan.canonical_neighbors.map(|site| self.site_block(site));
        crate::redstone::wire::get_raw_sides_from_neighbors(wire, &neighbors)
    }

    fn site_source_strength(&self, site: SiteId, resolved: &ResolvedSite) -> u8 {
        if let Some(CallbackReceiver::Component(node)) = resolved.receiver {
            if matches!(
                self.program.nodes()[node.index()].kind,
                CircuitNodeKind::Electrical(NodeType::Comparator { .. })
            ) {
                return self.node_state(node).output_strength;
            }
        }
        let fixed_component = matches!(resolved.receiver, Some(CallbackReceiver::Component(_)));
        let selectable_contact = matches!(
            self.program.sites()[site.index()].source,
            SiteSource::Contact(_)
        );
        if selectable_contact && !fixed_component {
            return self.contact_or_boundary_strength(site, resolved);
        }
        match resolved.block {
            Block::RedstoneTorch { lit: true } | Block::RedstoneWallTorch { lit: true, .. } => 15,
            Block::RedstoneBlock => 15,
            Block::StonePressurePlate { powered: true } => 15,
            block if block.pressure_plate_powered() == Some(true) => 15,
            Block::Lever { lever } if lever.powered => 15,
            Block::StoneButton { button } if button.powered => 15,
            Block::RedstoneRepeater { repeater } if repeater.powered => 15,
            Block::RedstoneComparator { .. } => self.program.sites()[site.index()].source_strength,
            Block::RedstoneWire { wire } => wire.power,
            Block::Observer { observer } if observer.powered => 15,
            _ if resolved.receiver.is_none() => self.program.sites()[site.index()].source_strength,
            _ => 0,
        }
    }

    fn power_no_dust(
        &self,
        site: SiteId,
        face: BlockFace,
        captured_emitter: Option<&ObservedSite>,
    ) -> u8 {
        let resolved = self
            .resolve_site(site, captured_emitter)
            .expect("wire query site was validated during preparation");
        if resolved.solid {
            return self.strong_power_at(site);
        }
        if matches!(resolved.block, Block::RedstoneWire { .. }) {
            return 0;
        }
        let strength = self.site_source_strength(site, &resolved);
        if strength > 0 && emits_weak(resolved.block, face) {
            strength
        } else {
            0
        }
    }

    fn strong_power_at(&self, site: SiteId) -> u8 {
        self.conducted_power(site, false)
    }

    fn capture_walk_site(&mut self, walk: &mut WireWalk, site: SiteId) {
        if walk.sites.contains_key(&site) {
            return;
        }
        let observed = self
            .resolve_site(site, None)
            .expect("wire traversal site was validated during preparation")
            .observed();
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire discover {:?} receiver={:?} properties={:?}",
                self.program.sites()[site.index()].position,
                observed.receiver,
                observed.properties
            )
        );
        walk.sites.insert(
            site,
            WalkSite {
                observed,
                visited: false,
                x_bias: 0,
                z_bias: 0,
                layer: 0,
                heading: None,
            },
        );
    }

    fn identify_wire_neighbors(&mut self, node: NodeId, walk: &mut WireWalk) -> [SiteId; 24] {
        let plan = self.wire_plan(node).clone();
        let root = self.component_sites[node.index()]
            .expect("wire root site was validated during preparation");
        self.capture_walk_site(walk, root);
        if let Some(heading) = walk.sites[&root].heading {
            return crate::redstone::wire::TURBO_ORDER[heading as usize]
                .map(|index| plan.canonical_neighbors[index]);
        }
        for site in plan.canonical_neighbors {
            self.capture_walk_site(walk, site);
        }

        let visited = plan
            .canonical_neighbors
            .map(|site| walk.sites[&site].visited);
        let from_west = visited[0] || visited[7] || visited[8];
        let from_east = visited[1] || visited[12] || visited[13];
        let from_north = visited[4] || visited[17] || visited[20];
        let from_south = visited[5] || visited[18] || visited[21];
        let mut x_bias = i32::from(from_west) - i32::from(from_east);
        let mut z_bias = i32::from(from_north) - i32::from(from_south);
        let (old_x_bias, old_z_bias) = {
            let root = &walk.sites[&root];
            (root.x_bias, root.z_bias)
        };
        let heading = if x_bias == 0 && z_bias == 0 {
            x_bias = old_x_bias;
            z_bias = old_z_bias;
            crate::redstone::wire::compute_heading(x_bias, z_bias)
        } else {
            if x_bias != 0 && z_bias != 0 {
                if old_x_bias != 0 {
                    z_bias = 0;
                }
                if old_z_bias != 0 {
                    x_bias = 0;
                }
            }
            crate::redstone::wire::compute_heading(x_bias, z_bias)
        };
        for site in plan.canonical_neighbors {
            let neighbor = walk.sites.get_mut(&site).unwrap();
            neighbor.x_bias = x_bias;
            neighbor.z_bias = z_bias;
        }
        walk.sites.get_mut(&root).unwrap().heading = Some(heading as u8);
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!("wire heading {node:?} bias=({x_bias},{z_bias}) order={heading}")
        );
        crate::redstone::wire::TURBO_ORDER[heading].map(|index| plan.canonical_neighbors[index])
    }

    fn wire_route_delivery(
        &self,
        route: RouteId,
        target: SiteId,
        observed: &ObservedSite,
    ) -> Option<RouteDelivery> {
        self.program.routes()[route.index()]
            .deliveries
            .iter()
            .copied()
            .find(|delivery| {
                delivery.target == target
                    && delivery.receiver.is_some()
                    && delivery.view == OperandView::Captured
                    && self.guard_matches_for_site(delivery.guard, target, observed)
            })
    }

    fn propagate_wire_changes(&mut self, node: NodeId, walk: &mut WireWalk, layer: u32) {
        let plan = self.wire_plan(node).clone();
        let Some(route) = plan.turbo_route else {
            return;
        };
        let ordered = self.identify_wire_neighbors(node, walk);
        for &site in &ordered {
            let next_layer = layer + 1;
            let changed = {
                let neighbor = walk.sites.get_mut(&site).unwrap();
                if next_layer > neighbor.layer {
                    neighbor.layer = next_layer;
                    true
                } else {
                    false
                }
            };
            if changed
                && self
                    .wire_route_delivery(route, site, &walk.sites[&site].observed)
                    .is_some()
            {
                walk.queues[1].push_back(site);
            }
        }
        for &site in &ordered[..4] {
            let next_layer = layer + 2;
            let changed = {
                let neighbor = walk.sites.get_mut(&site).unwrap();
                if next_layer > neighbor.layer {
                    neighbor.layer = next_layer;
                    true
                } else {
                    false
                }
            };
            if changed
                && self
                    .wire_route_delivery(route, site, &walk.sites[&site].observed)
                    .is_some()
            {
                walk.queues[2].push_back(site);
            }
        }
    }

    fn shift_wire_queues(&self, walk: &mut WireWalk) {
        walk.queues.rotate_left(1);
        walk.queues[2].clear();
    }

    fn evaluate_turbo_wire(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        let root = self.component_sites[node.index()]
            .expect("wire root site was validated during preparation");
        let mut walk = WireWalk::new();
        self.capture_walk_site(&mut walk, root);
        walk.sites.get_mut(&root).unwrap().visited = true;
        #[cfg(test)]
        graph_test_trace!(self, format!("wire turbo start {node:?}"));
        self.propagate_wire_changes(node, &mut walk, 0);
        self.shift_wire_queues(&mut walk);
        walk.layer = 1;
        while !walk.queues[0].is_empty() || !walk.queues[1].is_empty() {
            let count = walk.queues[0].len();
            for _ in 0..count {
                let site = walk.queues[0].pop_front().unwrap();
                self.dispatch_turbo_site(site, &mut walk)?;
            }
            self.shift_wire_queues(&mut walk);
            walk.layer += 1;
        }
        #[cfg(test)]
        graph_test_trace!(self, format!("wire turbo end {node:?}"));
        Ok(())
    }

    fn dispatch_turbo_site(
        &mut self,
        site: SiteId,
        walk: &mut WireWalk,
    ) -> Result<(), GraphExecutionError> {
        let observed = walk.sites[&site].observed.clone();
        let Some(receiver) = observed.receiver else {
            return Ok(());
        };
        self.counts.callbacks += 1;
        // Dust-to-dust steps stay inside the walk; native dispatches only the rest.
        #[cfg(test)]
        if self.test_recording
            && self.has_semantic_callback(receiver)
            && !matches!(receiver, CallbackReceiver::Component(node)
                if matches!(self.program.nodes()[node.index()].kind, CircuitNodeKind::Wire(_)))
        {
            self.observed_operations
                .push(GraphObservedOperation::Callback {
                    target: self.program.sites()[site.index()].position,
                    direction: None,
                });
        }
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire deliver {:?} layer={} Update {{ direction: None }} receiver={receiver:?}",
                self.program.sites()[site.index()].position,
                walk.layer
            )
        );
        match receiver {
            CallbackReceiver::Component(node)
                if matches!(
                    self.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Wire(_)
                ) =>
            {
                self.evaluate_turbo_wire_node(node, walk)
            }
            CallbackReceiver::Component(node) => self.update_component(
                node,
                DeliveryKind::Update { direction: None },
                Some(&observed),
            ),
            CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => {
                self.sample_piston(piston)
            }
            CallbackReceiver::Contact(_) => Ok(()),
        }
    }

    fn evaluate_turbo_wire_node(
        &mut self,
        node: NodeId,
        walk: &mut WireWalk,
    ) -> Result<(), GraphExecutionError> {
        let root = self.component_sites[node.index()]
            .expect("wire root site was validated during preparation");
        self.capture_walk_site(walk, root);
        walk.sites.get_mut(&root).unwrap().visited = true;
        let ordered = self.identify_wire_neighbors(node, walk);
        let plan = self.wire_plan(node).clone();
        let mut wire = match walk.sites[&root].observed.properties {
            ObservedProperties::Wire(wire) => wire,
            _ => unreachable!("captured wire site has non-wire properties"),
        };
        #[cfg(test)]
        let (new_power, block_power, wire_power) = self.turbo_wire_power(&plan, &ordered, walk);
        #[cfg(not(test))]
        let (new_power, _, _) = self.turbo_wire_power(&plan, &ordered, walk);
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire query {node:?} layer={} operands=captured-walk/live-strong block={block_power} direct={wire_power}",
                walk.layer,
            )
        );
        if wire.power == new_power {
            return Ok(());
        }
        #[cfg(test)]
        let old_power = wire.power;
        wire.power = new_power;
        walk.sites.get_mut(&root).unwrap().observed.properties = ObservedProperties::Wire(wire);
        self.set_wire(node, wire);
        #[cfg(test)]
        graph_test_trace!(
            self,
            format!(
                "wire strength {node:?} layer={} {old_power} -> {new_power}",
                walk.layer
            )
        );
        self.propagate_wire_changes(node, walk, walk.layer);
        Ok(())
    }

    fn turbo_wire_power(
        &self,
        plan: &WirePlan,
        ordered: &[SiteId; 24],
        walk: &WireWalk,
    ) -> (u8, u8, u8) {
        let mut wire_power = 0;
        for (face, index) in BlockFace::values().into_iter().zip([3, 2, 4, 5, 1, 0]) {
            let site = plan.canonical_neighbors[index];
            let observed = &walk.sites[&site].observed;
            wire_power = wire_power.max(self.power_no_dust(site, face, Some(observed)));
        }
        let mut block_power = 0;
        if wire_power < 15 {
            let center_up_solid = self.walk_site(ordered[1], walk).solid;
            let horizontal = [4, 5, 6, 7];
            let down = [8, 10, 12, 14];
            let up = [9, 11, 13, 15];
            for index in 0..4 {
                let neighbor = ordered[horizontal[index]];
                let neighbor_site = self.walk_site(neighbor, walk);
                block_power = self.walk_wire_strength(neighbor, walk, block_power);
                if !neighbor_site.solid {
                    block_power = self.walk_wire_strength(ordered[down[index]], walk, block_power);
                } else if !center_up_solid && !neighbor_site.transparent {
                    block_power = self.walk_wire_strength(ordered[up[index]], walk, block_power);
                }
            }
        }
        (
            wire_power.max(block_power.saturating_sub(1)),
            block_power,
            wire_power,
        )
    }

    fn walk_site(&self, site: SiteId, walk: &WireWalk) -> ResolvedSite {
        let observed = &walk.sites[&site].observed;
        self.resolve_site(site, Some(observed))
            .expect("captured wire site is valid")
    }

    fn walk_wire_strength(&self, site: SiteId, walk: &WireWalk, strength: u8) -> u8 {
        match walk.sites[&site].observed.properties {
            ObservedProperties::Wire(wire) => strength.max(wire.power),
            _ => strength,
        }
    }

    fn update_observer(
        &mut self,
        node: NodeId,
        direction: Option<BlockFace>,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
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

    fn update_torch(
        &mut self,
        node: NodeId,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
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

    fn update_repeater(
        &mut self,
        node: NodeId,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
        let current = self.node_state(node);
        let (powered, locked) = captured
            .and_then(|site| match site.properties {
                ObservedProperties::Electrical { powered, locked } => Some((powered, locked)),
                _ => None,
            })
            .unwrap_or((current.powered, current.repeater_locked));
        let should_lock = self.repeater_locked(node);
        if locked != should_lock {
            self.set_node(
                node,
                powered,
                u8::from(powered) * 15,
                Some(should_lock),
                false,
            )?;
        }
        if !should_lock && !self.is_pending(node) {
            let should_power = self.input_strength(node, InputPort::Main) > 0;
            if should_power != powered {
                let facing_diode = matches!(
                    self.program.nodes()[node.index()].kind,
                    CircuitNodeKind::Electrical(NodeType::Repeater {
                        facing_diode: true,
                        ..
                    })
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
            self.comparator_updates.push(self.position(node));
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
                CircuitNodeKind::Electrical(NodeType::Comparator {
                    facing_diode: true,
                    ..
                })
            );
            self.schedule(
                node,
                1,
                if facing_diode {
                    TickPriority::High
                } else {
                    TickPriority::Normal
                },
            );
        }
        Ok(())
    }

    fn tick_comparator(
        &mut self,
        node: NodeId,
        mode: ComparatorMode,
        _facing_diode: bool,
    ) -> Result<(), GraphExecutionError> {
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

    fn update_lamp(
        &mut self,
        node: NodeId,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
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

    fn input_strength(&self, node: NodeId, port: InputPort) -> u8 {
        self.query_power(PowerTarget::Input { node, port })
    }

    fn query_power(&self, target: PowerTarget) -> u8 {
        let mut maximum = 0;
        for link in self.program.power_links_to(target) {
            #[cfg(test)]
            self.power_links_examined
                .set(self.power_links_examined.get() + 1);
            if !self.guard_matches(link.guard, None) {
                continue;
            }
            maximum = maximum.max(self.link_strength(link));
        }
        maximum
    }

    fn link_strength(&self, link: &PowerLink) -> u8 {
        self.site_power(link.from, link.from_port)
            .saturating_sub(link.attenuation)
    }

    fn site_power(&self, site: SiteId, port: PowerPort) -> u8 {
        let Some(resolved) = self.resolve_site(site, None) else {
            return 0;
        };
        let stored = match resolved.receiver {
            Some(CallbackReceiver::Component(node)) => match &self.state.components[node.index()] {
                ComponentState::Electrical(state) => state.output_strength,
                ComponentState::Wire(wire) => wire.power,
                ComponentState::Specialized => 0,
            },
            Some(
                CallbackReceiver::PistonBase(_)
                | CallbackReceiver::PistonHead(_)
                | CallbackReceiver::Contact(_),
            )
            | None => self.contact_or_boundary_strength(site, &resolved),
        };
        match port {
            PowerPort::Output | PowerPort::Stored => stored,
            PowerPort::Weak(face) => {
                if self.emits_weak(site, resolved.block, face) {
                    stored
                } else {
                    0
                }
            }
            PowerPort::Strong(face) => {
                if self.emits_strong(site, resolved.block, face) {
                    stored
                } else {
                    0
                }
            }
            PowerPort::Conducted { include_dust } => self.conducted_power(site, include_dust),
        }
    }

    fn contact_or_boundary_strength(&self, site: SiteId, resolved: &ResolvedSite) -> u8 {
        if matches!(
            self.program.sites()[site.index()].source,
            SiteSource::Constant(_)
        ) {
            return self.program.sites()[site.index()].source_strength;
        }
        let Some(profile) = resolved.profile else {
            return 0;
        };
        match self.program.profiles()[profile.index()].electrical {
            ProfileElectrical::RedstoneSource => 15,
            ProfileElectrical::None | ProfileElectrical::Component => 0,
        }
    }

    fn emits_weak(&self, site: SiteId, block: Block, side: BlockFace) -> bool {
        let Block::RedstoneWire { wire } = block else {
            return emits_weak(block, side);
        };
        if side == BlockFace::Top {
            return true;
        }
        if side == BlockFace::Bottom {
            return false;
        }
        let SiteSource::Component(node) = self.program.sites()[site.index()].source else {
            // shortcut: moved wire contacts lack per-position plans until Step 08.
            return crate::redstone::wire::get_current_side(
                wire,
                side.unwrap_direction().opposite(),
            ) != RedstoneWireSide::None;
        };
        let CircuitNodeKind::Wire(_) = self.program.nodes()[node.index()].kind else {
            return crate::redstone::wire::get_current_side(
                wire,
                side.unwrap_direction().opposite(),
            ) != RedstoneWireSide::None;
        };
        let plan = self.wire_plan(node);
        let raw = self.raw_wire_sides(plan, wire);
        let regulated = crate::redstone::wire::regulate_sides(wire, raw);
        crate::redstone::wire::get_current_side(regulated, side.unwrap_direction().opposite())
            != RedstoneWireSide::None
    }

    fn emits_strong(&self, site: SiteId, block: Block, side: BlockFace) -> bool {
        if matches!(block, Block::RedstoneWire { .. }) {
            self.emits_weak(site, block, side)
        } else {
            emits_strong(block, side)
        }
    }

    fn conducted_power(&self, site: SiteId, include_dust: bool) -> u8 {
        self.query_power(PowerTarget::Conductor { site, include_dust })
    }

    fn guard_matches(&self, id: GuardId, captured: Option<&[Option<ObservedSite>]>) -> bool {
        self.guard_matches_using(id, captured, None)
    }

    fn guard_matches_for_site(&self, id: GuardId, site: SiteId, observed: &ObservedSite) -> bool {
        self.guard_matches_using(id, None, Some((site, observed)))
    }

    fn guard_matches_using(
        &self,
        id: GuardId,
        captured: Option<&[Option<ObservedSite>]>,
        captured_site: Option<(SiteId, &ObservedSite)>,
    ) -> bool {
        match &self.program.guards()[id.index()] {
            Guard::Always => true,
            Guard::State(test) => match *test {
                StateTest::PistonExtended(piston) => self.state.pistons[piston.index()].extended,
                StateTest::PistonHeadPresent(piston) => {
                    self.state.pistons[piston.index()].head_present
                }
                StateTest::PistonAvailability { piston, is } => {
                    self.state.pistons[piston.index()].availability == is
                }
                StateTest::ResourceAt { resource, contact } => {
                    self.state.resources[resource.index()].at == contact
                }
                StateTest::ResourceEnabled(resource) => {
                    self.state.resources[resource.index()].enabled
                }
            },
            Guard::Site { site, view, test } => {
                let resolved = match view {
                    OperandView::Live => self.resolve_site(*site, None),
                    OperandView::Captured => captured_site
                        .filter(|(captured_id, _)| captured_id == site)
                        .map(|(_, observed)| observed)
                        .or_else(|| {
                            captured
                                .and_then(|sites| sites.get(site.index()))
                                .and_then(Option::as_ref)
                        })
                        .and_then(|observed| self.resolve_observed_site(observed)),
                };
                let Some(resolved) = resolved else {
                    return false;
                };
                match test {
                    SiteTest::Solid => resolved.solid,
                    SiteTest::Transparent => resolved.transparent,
                    SiteTest::Cube => resolved.cube,
                    SiteTest::IsWire => matches!(resolved.block, Block::RedstoneWire { .. }),
                    SiteTest::Profile(profile) => resolved.profile == Some(*profile),
                }
            }
            Guard::Not(child) => !self.guard_matches_using(*child, captured, captured_site),
            Guard::All(children) => children
                .iter()
                .all(|child| self.guard_matches_using(*child, captured, captured_site)),
            Guard::Any(children) => children
                .iter()
                .any(|child| self.guard_matches_using(*child, captured, captured_site)),
        }
    }

    fn resolve_site(&self, site: SiteId, captured: Option<&ObservedSite>) -> Option<ResolvedSite> {
        if let Some(observed) = captured {
            return self.resolve_observed_site(observed);
        }
        let spec = *self.program.sites().get(site.index())?;
        match spec.source {
            SiteSource::Constant(profile) => Some(self.resolve_profile(profile, None)),
            SiteSource::Component(node) => {
                let block = self.node_block(node);
                Some(ResolvedSite {
                    profile: None,
                    receiver: Some(CallbackReceiver::Component(node)),
                    properties: self.node_properties(node),
                    block,
                    solid: block.is_solid(),
                    transparent: block.is_transparent(),
                    cube: block.is_cube(),
                })
            }
            SiteSource::Contact(contact) => {
                let spec = &self.program.contacts()[contact.index()];
                for variant in spec.variants.iter() {
                    if self.guard_matches(variant.when, None) {
                        return Some(self.resolve_profile(variant.profile, variant.receiver));
                    }
                }
                None
            }
        }
    }

    fn resolve_profile(
        &self,
        profile: ProfileId,
        receiver: Option<CallbackReceiver>,
    ) -> ResolvedSite {
        let facts = self.program.profiles()[profile.index()];
        let block = self
            .program
            .profile_block(profile)
            .expect("validated profile");
        ResolvedSite {
            profile: Some(profile),
            receiver,
            block,
            properties: self.contact_properties(receiver, block),
            solid: facts.solid,
            transparent: facts.transparent,
            cube: facts.cube,
        }
    }

    fn resolve_observed_site(&self, observed: &ObservedSite) -> Option<ResolvedSite> {
        let block = if let Some(receiver) = observed.receiver {
            match receiver {
                CallbackReceiver::Component(node) => {
                    self.node_block_with_properties(node, &observed.properties)
                }
                _ => self.block_for_profile(observed.profile?)?,
            }
        } else {
            self.block_for_profile(observed.profile?)?
        };
        let (solid, transparent, cube) = if let Some(profile) = observed.profile {
            let facts = self.program.profiles()[profile.index()];
            (facts.solid, facts.transparent, facts.cube)
        } else {
            (block.is_solid(), block.is_transparent(), block.is_cube())
        };
        Some(ResolvedSite {
            profile: observed.profile,
            receiver: observed.receiver,
            block,
            properties: observed.properties.clone(),
            solid,
            transparent,
            cube,
        })
    }

    fn block_for_profile(&self, profile: ProfileId) -> Option<Block> {
        self.program.profile_block(profile)
    }

    fn contact_properties(
        &self,
        receiver: Option<CallbackReceiver>,
        block: Block,
    ) -> ObservedProperties {
        match receiver {
            Some(CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston)) => {
                ObservedProperties::Piston {
                    extended: self.state.pistons[piston.index()].extended,
                }
            }
            _ => match block {
                Block::RedstoneWire { wire } => ObservedProperties::Wire(wire),
                _ => ObservedProperties::Passive,
            },
        }
    }

    fn node_properties(&self, node: NodeId) -> ObservedProperties {
        match &self.state.components[node.index()] {
            ComponentState::Electrical(state) if self.is_bulb(node) => {
                ObservedProperties::CopperBulb {
                    powered: state.powered,
                    lit: state.output_strength > 0,
                }
            }
            ComponentState::Electrical(state) => ObservedProperties::Electrical {
                powered: state.powered,
                locked: state.repeater_locked,
            },
            ComponentState::Wire(wire) => ObservedProperties::Wire(*wire),
            ComponentState::Specialized => match self.program.nodes()[node.index()].kind {
                CircuitNodeKind::Piston(piston) => ObservedProperties::Piston {
                    extended: self.state.pistons[piston.index()].extended,
                },
                _ => ObservedProperties::Passive,
            },
        }
    }

    fn node_block(&self, node: NodeId) -> Block {
        self.node_block_with_properties(node, &self.node_properties(node))
    }

    fn node_block_with_properties(&self, node: NodeId, properties: &ObservedProperties) -> Block {
        let mut block = self.program.nodes()[node.index()].block.unwrap();
        if matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Wire(_)
        ) {
            let wire = match properties {
                ObservedProperties::Wire(wire) => *wire,
                _ => self.wire_state(node),
            };
            return Block::RedstoneWire { wire };
        }
        let state = self.node_state(node);
        if block.is_copper_bulb() {
            let (powered, lit) = match properties {
                ObservedProperties::CopperBulb { powered, lit } => (*powered, *lit),
                _ => (state.powered, state.output_strength > 0),
            };
            return block.with_copper_bulb_state(lit, powered).unwrap();
        }
        match &mut block {
            Block::Lever { lever } => lever.powered = state.powered,
            Block::StoneButton { button } => button.powered = state.powered,
            Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => {
                *lit = state.powered
            }
            Block::RedstoneRepeater { repeater } => {
                repeater.powered = state.powered;
                repeater.locked = state.repeater_locked;
            }
            Block::RedstoneComparator { comparator } => comparator.powered = state.powered,
            Block::RedstoneLamp { lit } => *lit = state.powered,
            Block::Observer { observer } => observer.powered = state.powered,
            Block::NoteBlock {
                instrument,
                powered,
                ..
            } => {
                *powered = state.powered;
                if let CircuitNodeKind::Electrical(NodeType::NoteBlock {
                    instrument: initial_instrument,
                    ..
                }) = self.program.nodes()[node.index()].kind
                {
                    *instrument = state.note_instrument.unwrap_or(initial_instrument);
                }
            }
            Block::StonePressurePlate { powered } => *powered = state.powered,
            _ => {
                if let Some(updated) = block.with_pressure_plate_power(state.powered) {
                    block = updated;
                }
            }
        }
        if let ObservedProperties::Electrical { powered, locked } = properties {
            match &mut block {
                Block::RedstoneRepeater { repeater } => {
                    repeater.powered = *powered;
                    repeater.locked = *locked;
                }
                Block::RedstoneComparator { comparator } => comparator.powered = *powered,
                Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => {
                    *lit = *powered
                }
                Block::RedstoneLamp { lit } => *lit = *powered,
                Block::Observer { observer } => observer.powered = *powered,
                _ => {}
            }
        }
        block
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

    fn fire_route_unconditionally(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
        if let Some(route) = self.program.nodes()[node.index()].output_route {
            self.deliver_route(route, None)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    fn has_semantic_callback(&self, receiver: CallbackReceiver) -> bool {
        match receiver {
            CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) => true,
            CallbackReceiver::Component(node) => self.program.nodes()[node.index()]
                .block
                .is_some_and(|block| {
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
                            | "note_block"
                            | "iron_trapdoor"
                    )
                }),
            CallbackReceiver::Contact(_) => false,
        }
    }

    fn deliver_route(
        &mut self,
        route: RouteId,
        captured: Option<&[Option<ObservedSite>]>,
    ) -> Result<(), GraphExecutionError> {
        let delivery_count = self.program.routes()[route.index()].deliveries.len();
        for index in 0..delivery_count {
            let delivery = self.program.routes()[route.index()].deliveries[index];
            if !self.guard_matches(delivery.guard, captured) {
                continue;
            }
            let observed = if delivery.view == OperandView::Captured {
                captured
                    .and_then(|sites| sites.get(delivery.target.index()))
                    .and_then(Option::as_ref)
            } else {
                None
            };
            if delivery.view == OperandView::Captured && observed.is_none() {
                continue;
            }
            // Inert component updates still visit their no-op handler in native order.
            let receiver = delivery.receiver.or_else(|| {
                match (
                    self.program.sites()[delivery.target.index()].source,
                    delivery.kind,
                ) {
                    (SiteSource::Component(node), DeliveryKind::Update { .. }) => {
                        Some(CallbackReceiver::Component(node))
                    }
                    _ => None,
                }
            });
            let Some(receiver) = receiver else { continue };
            self.counts.callbacks += 1;
            #[cfg(test)]
            if self.test_recording
                && self.has_semantic_callback(receiver)
                && matches!(delivery.kind, DeliveryKind::Update { .. })
            {
                self.observed_operations
                    .push(GraphObservedOperation::Callback {
                        target: self.program.sites()[delivery.target.index()].position,
                        direction: match delivery.kind {
                            DeliveryKind::Update { direction } => direction,
                            DeliveryKind::Shape { direction } => Some(direction),
                        },
                    });
            }
            #[cfg(test)]
            graph_test_trace!(
                self,
                format!(
                    "route {}:{} {:?}",
                    route.index(),
                    delivery.sequence,
                    delivery.kind
                )
            );
            match receiver {
                CallbackReceiver::Component(node) => {
                    self.update_component(node, delivery.kind, observed)?;
                }
                CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston) => {
                    self.sample_piston(piston)?;
                }
                CallbackReceiver::Contact(_) => {}
            }
        }
        Ok(())
    }

    fn set_node(
        &mut self,
        node: NodeId,
        powered: bool,
        output_strength: u8,
        locked: Option<bool>,
        notify: bool,
    ) -> Result<(), GraphExecutionError> {
        let ComponentState::Electrical(state) = &mut self.state.components[node.index()] else {
            return Err(self.unsupported(node, "specialized component state"));
        };
        let changed = state.powered != powered
            || state.output_strength != output_strength
            || locked.is_some_and(|locked| state.repeater_locked != locked);
        state.powered = powered;
        state.output_strength = output_strength;
        state.pending_tick = false;
        if let Some(locked) = locked {
            state.repeater_locked = locked;
        }
        if changed {
            self.dirty[node.index()] = true;
            #[cfg(test)]
            graph_test_trace!(
                self,
                format!(
                    "state {:?} powered={powered} strength={output_strength}",
                    node
                )
            );
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
        self.program.nodes()[node.index()]
            .position
            .expect("component has a position")
    }

    fn is_bulb(&self, node: NodeId) -> bool {
        matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(NodeType::CopperBulb)
        )
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
                    CircuitNodeKind::Electrical(
                        NodeType::Torch | NodeType::Repeater { .. } | NodeType::Comparator { .. }
                    )
                ) {
                    return None;
                }
                let ComponentState::Electrical(state) = &self.state.components[index] else {
                    return None;
                };
                Some((node.position?, state.output_strength))
            })
            .collect()
    }

    pub(crate) fn inspect(&self, position: BlockPos) {
        match self.positions.get(&position) {
            Some(&node) => tracing::debug!(
                "Graph node {node:?}: {:?} {:?}",
                self.program.nodes()[node.index()].kind,
                self.state.components[node.index()]
            ),
            None => tracing::debug!("could not find graph node at {position:?}"),
        }
    }

    fn is_io(&self, node: NodeId) -> bool {
        matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Observer { .. }
                | CircuitNodeKind::Electrical(
                    NodeType::Lever
                        | NodeType::Button
                        | NodeType::PressurePlate
                        | NodeType::Lamp
                        | NodeType::CopperBulb
                        | NodeType::NoteBlock { .. }
                        | NodeType::CommandBlock { .. }
                )
        )
    }

    /// Presentation only: writes current graph state and drains effects. It
    /// reads no World state, so suppressed display cannot alter execution.
    pub(crate) fn flush<W: World>(&mut self, world: &mut W, io_only: bool) {
        for index in 0..self.dirty.len() {
            let node = NodeId::try_from_index(index).unwrap();
            // Hidden nodes stay dirty; reset materializes every node regardless.
            if !self.dirty[index] || (io_only && !self.is_io(node)) {
                continue;
            }
            self.write_node(world, node);
            self.dirty[index] = false;
        }
        if !io_only {
            for contact in 0..self.program.contacts().len() {
                let contact = ContactId::new(contact as u32);
                let position = self.program.contacts()[contact.index()].position;
                if self.cell_owned_by_node(position) {
                    continue;
                }
                // Clients see pending cells as empty; reset supplies interpreter motion.
                let block = match self.contact_block(contact) {
                    Block::MovingPiston { .. } => Block::Air,
                    block => block,
                };
                world.set_block(position, block);
            }
        }
        for effect in self.effects.drain(..) {
            match effect {
                PresentationEffect::BlockAction { .. } if io_only => {}
                PresentationEffect::BlockAction { position, action } => {
                    world.block_action(position, action)
                }
                PresentationEffect::Sound {
                    position,
                    id,
                    category,
                    volume,
                    pitch,
                } => world.play_sound(position, id, category, volume, pitch),
            }
        }
    }

    /// Whether a node's state owns its cell. Constants are compile-time
    /// templates; a moved redstone resource is owned by its contacts instead.
    fn owns_cell(&self, node: NodeId) -> bool {
        matches!(
            self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::Wire(_)
        ) || matches!(
            &self.program.nodes()[node.index()].kind,
            CircuitNodeKind::Electrical(kind) if *kind != NodeType::Constant
        )
    }

    fn cell_owned_by_node(&self, position: BlockPos) -> bool {
        self.positions
            .get(&position)
            .is_some_and(|&node| self.owns_cell(node))
    }

    fn write_node<W: World>(&self, world: &mut W, node: NodeId) {
        let spec = &self.program.nodes()[node.index()];
        let Some(position) = spec.position else {
            return;
        };
        if !self.owns_cell(node) {
            return;
        }
        world.set_block(position, self.node_block(node));
        if matches!(
            spec.kind,
            CircuitNodeKind::Electrical(NodeType::Comparator { .. })
        ) {
            world.set_block_entity(
                position,
                BlockEntity::Comparator {
                    output_strength: self.node_state(node).output_strength,
                },
            );
        }
        if matches!(
            spec.kind,
            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })
        ) && let Some(entity) = self.node_state(node).command_entity.clone()
        {
            world.set_block_entity(position, BlockEntity::CommandBlock(Box::new(entity)));
        }
    }

    /// Hand the current graph future back to the interpreter at a tick
    /// boundary and drop graph-owned pending work. Pending transitions are
    /// reproduced, never settled.
    pub(crate) fn reset<W: World>(&mut self, world: &mut W) {
        self.flush(world, false);
        self.project_future(world);
        self.dirty.fill(false);
        self.scheduler.clear();
        self.pending.clear();
        self.piston_requests.clear();
        self.deferred_pistons.clear();
    }

    /// Write the interpreter state equivalent to the current graph future:
    /// component and contact blocks, motions derived from live deferred
    /// events, ordinary scheduled ticks, queued requests, and the clock.
    /// Reads no World state and changes no graph state.
    pub(crate) fn project_future<W: World>(&self, world: &mut W) {
        debug_assert_eq!(self.clock.phase, GraphPhase::BetweenTicks);
        let interpreter = world.piston_state_mut();
        interpreter.logical_tick = self.clock.logical_tick;
        interpreter.phase = mchprs_world::AdvancePhase::BetweenTicks;
        interpreter.scheduled_advanced = false;
        interpreter.events.clear();
        interpreter.movement_work.clear();
        interpreter.movement_cursor = 0;

        for index in 0..self.program.nodes().len() {
            self.write_node(world, NodeId::try_from_index(index).unwrap());
        }
        let motions = self.pending_motions();
        for index in 0..self.program.contacts().len() {
            let contact = ContactId::new(index as u32);
            let position = self.program.contacts()[index].position;
            if self.cell_owned_by_node(position)
                || motions.iter().any(|motion| motion.position == position)
            {
                continue;
            }
            let block = self.contact_block(contact);
            debug_assert!(
                !matches!(block, Block::MovingPiston { .. }),
                "pending contact at {position:?} has no live deferred event"
            );
            world.set_block(position, block);
        }
        for motion in &motions {
            motion.materialize(world);
        }

        for (tick, delay, priority) in self.scheduler.iter() {
            world.restore_scheduled_tick(TickEntry {
                pos: tick.position,
                block_type: tick.block_type,
                ticks_left: delay as u32,
                tick_priority: priority,
            });
        }
        for request in &self.piston_requests {
            let spec = self.program.pistons()[request.piston.index()];
            world.enqueue_piston_event(PistonEvent {
                pos: self.program.contacts()[spec.base.index()].position,
                sticky: spec.sticky,
                facing: spec.facing,
                action: request.action,
            });
        }
    }

    /// Interpreter motions implied by live deferred events, in native creation
    /// order. Stale actor or resource parts were interrupted or replaced and
    /// have no interpreter motion.
    fn pending_motions(&self) -> Vec<MotionProjection> {
        let mut motions = Vec::new();
        for event in &self.deferred_pistons {
            let spec = self.program.pistons()[event.piston.index()];
            let piston = RedstonePiston {
                facing: spec.facing.into(),
                sticky: spec.sticky,
                extended: false,
            };
            let remaining = event.due_tick.saturating_sub(self.clock.logical_tick);
            debug_assert!(
                (1..=2).contains(&remaining),
                "deferred event outside its window"
            );
            let position = |contact: ContactId| self.program.contacts()[contact.index()].position;
            let actor =
                (self.state.pistons[event.piston.index()].revision == event.revision).then(|| {
                    MotionProjection {
                        position: position(if event.extending {
                            spec.near
                        } else {
                            spec.base
                        }),
                        actor: piston,
                        carried: if event.extending {
                            Block::PistonHead {
                                head: piston.into(),
                            }
                        } else {
                            Block::Piston { piston }
                        },
                        extending: event.extending,
                        source: true,
                        remaining,
                    }
                });
            let resource = event.resource.and_then(|transfer| {
                let state = self.state.resources[transfer.resource.index()];
                (state.revision == transfer.revision
                    && state.at == transfer.contact
                    && !state.enabled)
                    .then(|| MotionProjection {
                        position: position(transfer.contact),
                        actor: piston,
                        carried: self
                            .program
                            .profile_block(
                                self.program.resources()[transfer.resource.index()].material,
                            )
                            .expect("resource material profile was validated"),
                        extending: event.extending,
                        source: false,
                        remaining,
                    })
            });
            // Native extension installs the payload before the head; retraction
            // moves the base before the pulled payload.
            if event.extending {
                motions.extend(resource);
                motions.extend(actor);
            } else {
                motions.extend(actor);
                motions.extend(resource);
            }
        }
        motions
    }

    #[cfg(test)]
    fn trace(&self) -> &[String] {
        &self.trace
    }

    #[cfg(test)]
    pub(crate) fn clear_test_traces(&mut self) {
        self.trace.clear();
        self.callback_order.clear();
        self.comparator_updates.clear();
        self.observed_operations.clear();
    }

    #[cfg(test)]
    pub(crate) fn set_test_recording(&mut self, enabled: bool) {
        self.test_recording = enabled;
        if !enabled {
            self.clear_test_traces();
        }
    }

    #[cfg(test)]
    fn scheduled_ticks(&self) -> Vec<TickEntry> {
        self.scheduler
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

/// Boundary metadata for one interpreter motion. Derived only during reset
/// from an event deadline; graph execution never stores progress.
struct MotionProjection {
    position: BlockPos,
    actor: RedstonePiston,
    carried: Block,
    extending: bool,
    source: bool,
    /// Ticks until the event publishes; the interpreter restores on the
    /// movement visit after progress reaches 1.
    remaining: u64,
}

impl MotionProjection {
    fn materialize<W: World>(&self, world: &mut W) {
        let advances = 3.0 - self.remaining as f32;
        let progress = (advances * 0.5).min(1.0);
        let previous_progress = (advances - 1.0) * 0.5;
        world.set_block(
            self.position,
            Block::MovingPiston {
                moving: self.actor.into(),
            },
        );
        let mut entity = MovingPistonEntity {
            facing: self.actor.facing.into(),
            progress: 0,
            block_state: self.carried.get_id(),
            extending: self.extending,
            source: self.source,
        };
        entity.set_progress(previous_progress);
        world.set_block_entity(self.position, BlockEntity::MovingPiston(entity));
        // Worlds that do not register motions with their entities get one here.
        let index = world.piston_motion_index(self.position, None);
        let state = world.piston_state_mut();
        let index = index.unwrap_or_else(|| {
            state.next_identity += 1;
            state.motions.push_back(mchprs_world::PistonMotion {
                pos: self.position,
                identity: state.next_identity,
                progress,
                previous_progress,
                last_tick: state.logical_tick,
                carried_entity: None,
            });
            state.motions.len() - 1
        });
        let motion = &mut state.motions[index];
        motion.progress = progress;
        motion.previous_progress = previous_progress;
        motion.last_tick = state.logical_tick;
    }
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "graph_fuzz_tests.rs"]
mod fuzz_tests;

#[cfg(test)]
#[path = "graph_test_family_tests.rs"]
mod test_family_tests;

#[cfg(test)]
#[path = "graph_adder_tests.rs"]
mod adder_tests;

fn emits_weak(block: Block, side: BlockFace) -> bool {
    match block {
        Block::RedstoneTorch { .. } => side != BlockFace::Top,
        Block::RedstoneWallTorch { facing, .. } => facing.block_face() != side,
        Block::RedstoneBlock
        | Block::Lever { .. }
        | Block::StoneButton { .. }
        | Block::StonePressurePlate { .. } => true,
        block if block.pressure_plate_powered().is_some() => true,
        Block::RedstoneRepeater { repeater } => repeater.facing.block_face() == side,
        Block::RedstoneComparator { comparator } => comparator.facing.block_face() == side,
        Block::Observer { observer } => observer.facing == side.into(),
        Block::RedstoneWire { wire } => match side {
            BlockFace::Top => true,
            BlockFace::Bottom => false,
            _ => {
                crate::redstone::wire::get_current_side(wire, side.unwrap_direction().opposite())
                    != RedstoneWireSide::None
            }
        },
        _ => false,
    }
}

fn emits_strong(block: Block, side: BlockFace) -> bool {
    use mchprs_blocks::blocks::{ButtonFace, LeverFace};
    match block {
        Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. } => side == BlockFace::Bottom,
        Block::Lever { lever } => match side {
            BlockFace::Top => lever.face == LeverFace::Floor,
            BlockFace::Bottom => lever.face == LeverFace::Ceiling,
            _ => lever.face == LeverFace::Wall && lever.facing == side.unwrap_direction(),
        },
        Block::StoneButton { button } => match side {
            BlockFace::Top => button.face == ButtonFace::Floor,
            BlockFace::Bottom => button.face == ButtonFace::Ceiling,
            _ => button.face == ButtonFace::Wall && button.facing == side.unwrap_direction(),
        },
        Block::StonePressurePlate { .. } => side == BlockFace::Top,
        block if block.pressure_plate_powered().is_some() => side == BlockFace::Top,
        Block::RedstoneWire { .. }
        | Block::RedstoneRepeater { .. }
        | Block::RedstoneComparator { .. }
        | Block::Observer { .. } => emits_weak(block, side),
        _ => false,
    }
}
