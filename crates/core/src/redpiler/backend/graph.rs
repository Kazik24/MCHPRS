#![allow(dead_code)] // Kept internal until the public activation step.

//! Graph-native execution for fixed components. This backend is test-targeted
//! until activation, dust traversal, and piston transitions are reviewed.

use super::TickScheduler;
use crate::redpiler::TaskMonitor;
use crate::redpiler::analysis::{AnalysisLimits, callbacks, electrical, geometry};
use crate::redpiler::compile_graph::{NodeState, NodeType};
use crate::redpiler::program_graph::*;
use crate::world::World;
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ComparatorMode, RedstoneWire, RedstoneWireSide};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{TickEntry, TickPriority};
use rustc_hash::FxHashMap;
#[cfg(test)]
use std::cell::Cell;
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

const SCHEDULER_BUCKETS: usize = super::queue::NUM_QUEUES;

#[derive(Debug)]
pub(crate) enum GraphBackendError {
    Preparation(String),
    Graph(GraphBuildError),
    MissingBlockState {
        node: NodeId,
    },
    MissingComponentSite {
        node: NodeId,
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
    UnsupportedComponent {
        position: Option<BlockPos>,
        kind: &'static str,
    },
}

impl fmt::Display for GraphBackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preparation(error) => f.write_str(error),
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
            Self::UnsupportedComponent { position, kind } => {
                write!(f, "graph execution does not support {kind} at {position:?}")
            }
        }
    }
}

impl std::error::Error for GraphBackendError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ScheduledNodeTick {
    node: Option<NodeId>,
    position: BlockPos,
    registry_type: u32,
    current_registry_type: u32,
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
    resource_revision: Option<u64>,
    due_tick: u64,
    extending: bool,
}

#[derive(Debug, Clone, Copy)]
enum PresentationEffect {
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
}

impl fmt::Display for GraphExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedComponent { position, kind } => {
                write!(f, "graph execution does not support {kind} at {position:?}")
            }
        }
    }
}

impl std::error::Error for GraphExecutionError {}

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
    pending: FxHashMap<ScheduledNodeTick, usize>,
    piston_requests: VecDeque<PistonRequest>,
    deferred_pistons: VecDeque<DeferredPistonEvent>,
    clock: Clock,
    dirty: Vec<bool>,
    effects: Vec<PresentationEffect>,
    #[cfg(test)]
    trace: Vec<String>,
    #[cfg(test)]
    callback_order: Vec<(BlockPos, DeliveryKind)>,
    #[cfg(test)]
    comparator_updates: Vec<BlockPos>,
    #[cfg(test)]
    power_links_examined: Cell<usize>,
}

impl GraphBackend {
    pub(crate) fn compile<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
    ) -> Result<Self, GraphBackendError> {
        let limits = AnalysisLimits::for_budget(monitor.budget_multiplier());
        let geometry = geometry::prepare_geometry(world, bounds, &ticks, &monitor, limits)
            .map_err(|error| GraphBackendError::Preparation(error.to_string()))?;
        let mut electrical = electrical::prepare_electrical(
            world,
            geometry.report.bounds,
            &geometry.draft,
            &monitor,
        )
        .map_err(|error| GraphBackendError::Preparation(error.to_string()))?;
        let callbacks =
            callbacks::prepare_callbacks(world, &geometry.draft, &mut electrical, &monitor)
                .map_err(|error| GraphBackendError::Preparation(error.to_string()))?;
        if monitor.cancelled() {
            return Err(GraphBackendError::Preparation(
                "graph preparation cancelled".into(),
            ));
        }

        let power_links = electrical
            .links
            .iter()
            .map(|link| PowerLink {
                from: link.from,
                from_port: link.from_port,
                to: match link.to {
                    electrical::ElectricalTarget::Input { node, port } => {
                        PowerTarget::Input { node, port }
                    }
                    electrical::ElectricalTarget::Conductor { site, include_dust } => {
                        PowerTarget::Conductor { site, include_dust }
                    }
                },
                face: link.face,
                attenuation: link.attenuation,
                guard: link.guard,
            })
            .collect();
        let draft = CircuitDraft {
            nodes: electrical.nodes,
            links: callbacks.links,
            power_links,
            comparator_inputs: electrical.comparators,
            profiles: electrical.profiles,
            guards: electrical.guards,
            contacts: electrical.contacts,
            routes: callbacks.routes,
            wires: electrical.wires,
            sites: electrical.sites,
            pistons: callbacks.pistons,
            resources: geometry
                .draft
                .resources
                .iter()
                .map(|resource| resource.spec.clone())
                .collect(),
            initial_pistons: geometry.draft.initial_pistons,
            initial_resources: geometry.draft.initial_resources,
        };
        let (program, state) = draft.lower().map_err(GraphBackendError::Graph)?;
        Self::validate_supported(&program)?;
        let mut backend = Self::new(program, state, world.piston_state().logical_tick)?;
        backend.import_ticks(world, ticks)?;
        if monitor.cancelled() {
            return Err(GraphBackendError::Preparation(
                "graph preparation cancelled".into(),
            ));
        }
        Ok(backend)
    }

    fn validate_supported(program: &CircuitProgram) -> Result<(), GraphBackendError> {
        for node in program.nodes() {
            let unsupported = match node.kind {
                CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => Some("command block"),
                CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => Some("note block"),
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
        let mut resource_owners = vec![0usize; program.resources().len()];
        for piston in program.pistons() {
            let PistonLane::SingleResource { far } = piston.lane else {
                continue;
            };
            if let Some((index, _)) =
                program
                    .resources()
                    .iter()
                    .enumerate()
                    .find(|(_, resource)| {
                        resource.contacts.contains(&piston.near) && resource.contacts.contains(&far)
                    })
            {
                resource_owners[index] += 1;
                if resource_owners[index] > 1 {
                    return Err(GraphBackendError::UnsupportedComponent {
                        position: program.nodes()[piston.node.index()].position,
                        kind: "shared piston resource",
                    });
                }
            }
        }
        for site in program.sites() {
            let SiteSource::Constant(profile) = site.source else {
                continue;
            };
            if !matches!(
                program.profile_block(profile),
                Some(Block::RedstoneWire { .. })
            ) {
                continue;
            }
            let adjacent_consumer = program.nodes().iter().any(|node| {
                node.position.is_some_and(|position| {
                    BlockFace::values()
                        .into_iter()
                        .any(|face| site.position.offset(face) == position)
                }) && matches!(
                    node.kind,
                    CircuitNodeKind::Electrical(_)
                        | CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::Piston(_)
                )
            });
            if adjacent_consumer {
                return Err(GraphBackendError::UnsupportedComponent {
                    position: Some(site.position),
                    kind: "external redstone wire boundary",
                });
            }
        }
        Ok(())
    }

    fn new(
        program: CircuitProgram,
        mut state: CircuitState,
        logical_tick: u64,
    ) -> Result<Self, GraphBackendError> {
        let mut positions = FxHashMap::default();
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
                if let Some(position) = node.position {
                    positions.insert(position, id);
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
            #[cfg(test)]
            trace: Vec::new(),
            #[cfg(test)]
            callback_order: Vec::new(),
            #[cfg(test)]
            comparator_updates: Vec::new(),
            #[cfg(test)]
            power_links_examined: Cell::new(0),
        })
    }

    fn import_ticks<W: World>(
        &mut self,
        world: &W,
        ticks: Vec<TickEntry>,
    ) -> Result<(), GraphBackendError> {
        for tick in ticks {
            if tick.ticks_left as usize >= SCHEDULER_BUCKETS {
                return Err(GraphBackendError::TickOutsideScheduler {
                    delay: tick.ticks_left,
                });
            }
            if tick.ticks_left == 0 && tick.tick_priority != TickPriority::NanoTick {
                return Err(GraphBackendError::InvalidZeroDelayPriority {
                    priority: tick.tick_priority,
                });
            }
            let node = self.positions.get(&tick.pos).copied();
            let current_registry_type = node
                .map(|node| self.registry_type(node))
                .unwrap_or_else(|| world.get_block(tick.pos).registry_id());
            let registry_type = tick.block_type.unwrap_or(current_registry_type);
            if node.is_none()
                && registry_type == current_registry_type
                && world.get_block(tick.pos) != Block::Air
            {
                return Err(GraphBackendError::UnsupportedTickTarget {
                    position: tick.pos,
                    block_type: current_registry_type,
                });
            }
            let scheduled = ScheduledNodeTick {
                node,
                position: tick.pos,
                registry_type,
                current_registry_type,
            };
            self.scheduler.schedule_half_tick(
                scheduled,
                tick.ticks_left as usize,
                tick.tick_priority,
            );
            *self.pending.entry(scheduled).or_default() += 1;
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
        let tick = ScheduledNodeTick {
            node: Some(node),
            position: self.position(node),
            registry_type: self.registry_type(node),
            current_registry_type: self.registry_type(node),
        };
        if self.pending.contains_key(&tick) {
            return;
        }
        self.scheduler.schedule_tick(tick, delay, priority);
        self.pending.insert(tick, 1);
        #[cfg(test)]
        self.trace
            .push(format!("schedule {:?} +{delay} {priority:?}", node));
    }

    fn is_pending(&self, node: NodeId) -> bool {
        self.pending.contains_key(&ScheduledNodeTick {
            node: Some(node),
            position: self.position(node),
            registry_type: self.registry_type(node),
            current_registry_type: self.registry_type(node),
        })
    }

    fn popped(&mut self, tick: ScheduledNodeTick) {
        if let Some(count) = self.pending.get_mut(&tick) {
            *count -= 1;
            if *count == 0 {
                self.pending.remove(&tick);
            }
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), GraphExecutionError> {
        self.clock.logical_tick += 1;
        self.clock.phase = GraphPhase::ScheduledTicks;
        self.clock.scheduled_advanced = false;
        #[cfg(test)]
        self.trace.push("phase scheduled_ticks".into());
        if let Err(error) = self.drain_current_ticks() {
            self.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }

        self.scheduler.end_last_tick_move_next();
        self.clock.scheduled_advanced = true;
        #[cfg(test)]
        self.trace.push("clock.advance".into());
        #[cfg(test)]
        self.trace
            .push("phase scheduled_ticks_after_advance".into());
        if let Err(error) = self.drain_current_ticks() {
            self.clock.phase = GraphPhase::BetweenTicks;
            self.clock.scheduled_advanced = false;
            return Err(error);
        }

        self.clock.phase = GraphPhase::PistonActions;
        #[cfg(test)]
        self.trace.push("phase piston_actions".into());
        if let Err(error) = self.drain_piston_actions() {
            self.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }
        self.clock.phase = GraphPhase::DeferredConnections;
        #[cfg(test)]
        self.trace.push("phase deferred_connections".into());
        if let Err(error) = self.publish_deferred_pistons() {
            self.clock.phase = GraphPhase::BetweenTicks;
            return Err(error);
        }
        self.clock.phase = GraphPhase::BetweenTicks;
        self.clock.scheduled_advanced = false;
        #[cfg(test)]
        self.trace.push("phase between_ticks".into());
        Ok(())
    }

    fn drain_current_ticks(&mut self) -> Result<(), GraphExecutionError> {
        while let Some(tick) = self.scheduler.this_tick().pop_first() {
            self.popped(tick);
            if tick.registry_type != tick.current_registry_type {
                #[cfg(test)]
                self.trace
                    .push(format!("stale {:?} skipped", tick.position));
                continue;
            }
            let Some(node) = tick.node else {
                #[cfg(test)]
                self.trace
                    .push(format!("unbound {:?} skipped", tick.position));
                continue;
            };
            #[cfg(test)]
            self.trace.push(format!("dispatch {node:?}"));
            self.tick_component(node)?;
        }
        Ok(())
    }

    pub(crate) fn use_block(&mut self, position: BlockPos) -> Result<(), GraphExecutionError> {
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

    fn tick_component(&mut self, node: NodeId) -> Result<(), GraphExecutionError> {
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
                Err(self.unsupported(node, "command block"))
            }
            CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => {
                Err(self.unsupported(node, "note block"))
            }
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
        self.callback_order.push((self.position(node), kind));
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
                Err(self.unsupported(node, "command block"))
            }
            CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => {
                Err(self.unsupported(node, "note block"))
            }
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

    fn sample_piston(&mut self, piston: PistonId) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let powered = self.input_strength(spec.node, InputPort::Main) > 0
            || self.input_strength(spec.node, InputPort::Quasi) > 0;
        let state = self.state.pistons[piston.index()];
        if state.availability == PistonAvailability::BasePending {
            return Ok(());
        }
        let action = match (powered, state.extended) {
            (true, false) => mchprs_world::PistonAction::Extend,
            (false, true) => {
                if state.availability == PistonAvailability::HeadPending
                    && matches!(spec.lane, PistonLane::SingleResource { .. })
                {
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
            self.trace.push(format!(
                "piston request {:?} {:?}",
                request.piston, request.action
            ));
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
        let powered = self.input_strength(spec.node, InputPort::Main) > 0
            || self.input_strength(spec.node, InputPort::Quasi) > 0;
        match request.action {
            mchprs_world::PistonAction::Extend
                if powered
                    && !state.extended
                    && state.availability == PistonAvailability::Ready =>
            {
                #[cfg(test)]
                self.trace.push(format!(
                    "piston accept {:?} {:?}",
                    request.piston, request.action
                ));
                self.extend_connections(request.piston)?;
            }
            mchprs_world::PistonAction::Retract
            | mchprs_world::PistonAction::RetractWithoutPull
                if !powered && state.extended =>
            {
                #[cfg(test)]
                self.trace.push(format!(
                    "piston accept {:?} {:?}",
                    request.piston, request.action
                ));
                self.retract_connections(
                    request.piston,
                    request.action == mchprs_world::PistonAction::RetractWithoutPull,
                )?;
            }
            _ => {
                #[cfg(test)]
                self.trace.push(format!(
                    "piston cancelled {:?} {:?}",
                    request.piston, request.action
                ));
            }
        }
        Ok(())
    }

    fn set_resource_contact(
        &mut self,
        piston: PistonId,
        contact: ContactId,
        enabled: bool,
    ) -> Option<(ResourceId, u64)> {
        let spec = self.program.pistons()[piston.index()];
        let PistonLane::SingleResource { far } = spec.lane else {
            return None;
        };
        let (index, _) = self
            .program
            .resources()
            .iter()
            .enumerate()
            .find(|(_, resource)| {
                resource.contacts.contains(&spec.near) && resource.contacts.contains(&far)
            })?;
        let state = &mut self.state.resources[index];
        state.at = contact;
        state.enabled = enabled;
        state.revision = state.revision.wrapping_add(1);
        Some((ResourceId::new(index as u32), state.revision))
    }

    fn release_resource(
        &mut self,
        piston: PistonId,
        contact: ContactId,
    ) -> Option<(ResourceId, u64)> {
        self.set_resource_contact(piston, contact, false)
    }

    fn extend_connections(&mut self, piston: PistonId) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let revision = {
            let state = &mut self.state.pistons[piston.index()];
            state.revision = state.revision.wrapping_add(1);
            state.availability = PistonAvailability::PublishingHeadRemoval;
            state.revision
        };
        let resource = match spec.lane {
            PistonLane::SingleResource { far } => self.release_resource(piston, far),
            PistonLane::Empty => None,
        };
        if let Some(route) = spec.routes.far_changed {
            self.deliver_route(route, None)?;
        }
        self.deliver_route(spec.routes.near_changed, None)?;
        self.state.pistons[piston.index()].extended = true;
        self.state.pistons[piston.index()].availability = PistonAvailability::HeadPending;
        self.deliver_route(spec.routes.base_changed, None)?;
        let resource_revision = resource.map(|(_, revision)| revision);
        self.deferred_pistons.push_back(DeferredPistonEvent {
            piston,
            revision,
            resource_revision,
            due_tick: self.clock.logical_tick + 2,
            extending: true,
        });
        #[cfg(test)]
        self.trace
            .push(format!("piston extend {:?} revision={revision}", piston));
        Ok(())
    }

    fn retract_connections(
        &mut self,
        piston: PistonId,
        without_pull: bool,
    ) -> Result<(), GraphExecutionError> {
        let spec = self.program.pistons()[piston.index()];
        let revision = {
            let state = &mut self.state.pistons[piston.index()];
            state.revision = state.revision.wrapping_add(1);
            state.availability = PistonAvailability::BasePending;
            state.revision
        };
        self.deliver_route(spec.routes.base_changed, None)?;
        let resource = match spec.lane {
            PistonLane::SingleResource { far } if without_pull => {
                self.set_resource_contact(piston, far, true);
                if let Some(route) = spec.routes.resource_restored_far {
                    self.deliver_route(route, None)?;
                }
                None
            }
            PistonLane::SingleResource { .. } => self.release_resource(piston, spec.near),
            PistonLane::Empty => None,
        };
        if !without_pull {
            if let Some(route) = spec.routes.far_changed {
                self.deliver_route(route, None)?;
            }
        }
        self.deliver_route(spec.routes.near_changed, None)?;
        self.deferred_pistons.push_back(DeferredPistonEvent {
            piston,
            revision,
            resource_revision: resource.map(|(_, revision)| revision),
            due_tick: self.clock.logical_tick + 2,
            extending: false,
        });
        #[cfg(test)]
        self.trace
            .push(format!("piston retract {:?} revision={revision}", piston));
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
        if state.revision != event.revision {
            #[cfg(test)]
            self.trace.push(format!(
                "piston stale {:?} revision={}",
                event.piston, event.revision
            ));
            return Ok(());
        }
        let spec = self.program.pistons()[event.piston.index()];
        if let (PistonLane::SingleResource { far }, Some(expected)) =
            (spec.lane, event.resource_revision)
        {
            let contact = if event.extending { far } else { spec.near };
            let Some(resource_index) = self.program.resources().iter().position(|resource| {
                resource.contacts.contains(&spec.near) && resource.contacts.contains(&far)
            }) else {
                return Ok(());
            };
            if self.state.resources[resource_index].revision != expected {
                #[cfg(test)]
                self.trace.push(format!(
                    "resource stale {:?} revision={expected}",
                    event.piston
                ));
                return Ok(());
            }
            if let Some((resource, revision)) =
                self.set_resource_contact(event.piston, contact, true)
            {
                debug_assert_eq!(revision, expected.wrapping_add(1));
                let route = if event.extending {
                    spec.routes.resource_restored_far
                } else {
                    spec.routes.resource_restored_near
                };
                if let Some(route) = route {
                    self.deliver_route(route, None)?;
                }
                let _ = resource;
            }
        }
        if event.extending {
            self.state.pistons[event.piston.index()].availability = PistonAvailability::Ready;
            self.deliver_route(spec.routes.head_restored, None)?;
        } else {
            self.state.pistons[event.piston.index()].extended = false;
            self.state.pistons[event.piston.index()].availability = PistonAvailability::Ready;
            self.deliver_route(spec.routes.base_restored, None)?;
            self.deliver_route(spec.routes.head_restored, None)?;
        }
        #[cfg(test)]
        self.trace.push(format!(
            "piston restored {:?} revision={}",
            event.piston, event.revision
        ));
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
        self.trace
            .push(format!("wire state {node:?} {old:?} -> {wire:?}"));
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
        self.trace.push(format!(
            "wire seed {node:?} operands=live-sites/live-strong block={block_power} wire={wire_power} power {} -> {power}",
            wire.power,
        ));
        if wire.power == power {
            return Ok(());
        }
        wire.power = power;
        self.set_wire(node, wire);
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
        self.trace.push(format!(
            "wire discover {:?} receiver={:?} properties={:?}",
            self.program.sites()[site.index()].position,
            observed.receiver,
            observed.properties
        ));
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
        self.trace.push(format!(
            "wire heading {node:?} bias=({x_bias},{z_bias}) order={heading}"
        ));
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
        self.trace.push(format!("wire turbo start {node:?}"));
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
        self.trace.push(format!("wire turbo end {node:?}"));
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
        #[cfg(test)]
        self.trace.push(format!(
            "wire deliver {:?} layer={} Update {{ direction: None }} receiver={receiver:?}",
            self.program.sites()[site.index()].position,
            walk.layer
        ));
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
        self.trace.push(format!(
            "wire query {node:?} layer={} operands=captured-walk/live-strong block={block_power} direct={wire_power}",
            walk.layer,
        ));
        if wire.power == new_power {
            return Ok(());
        }
        #[cfg(test)]
        let old_power = wire.power;
        wire.power = new_power;
        walk.sites.get_mut(&root).unwrap().observed.properties = ObservedProperties::Wire(wire);
        self.set_wire(node, wire);
        #[cfg(test)]
        self.trace.push(format!(
            "wire strength {node:?} layer={} {old_power} -> {new_power}",
            walk.layer
        ));
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
        self.comparator_updates.push(self.position(node));
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
        let Some(plan) = self
            .program
            .comparator_inputs()
            .iter()
            .find(|plan| plan.node == node)
        else {
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

    fn node_state(&self, node: NodeId) -> NodeState {
        match &self.state.components[node.index()] {
            ComponentState::Electrical(state) => state.clone(),
            _ => NodeState::default(),
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

    fn deliver_route(
        &mut self,
        route: RouteId,
        captured: Option<&[Option<ObservedSite>]>,
    ) -> Result<(), GraphExecutionError> {
        let deliveries = self.program.routes()[route.index()].deliveries.to_vec();
        for delivery in deliveries {
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
            let receiver = self
                .resolve_site(delivery.target, observed)
                .and_then(|site| site.receiver);
            let Some(receiver) = receiver else { continue };
            #[cfg(test)]
            self.trace.push(format!(
                "route {}:{} {:?}",
                route.index(),
                delivery.sequence,
                delivery.kind
            ));
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
            self.trace.push(format!(
                "state {:?} powered={powered} strength={output_strength}",
                node
            ));
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

    pub(crate) fn flush<W: World>(&mut self, world: &mut W) {
        for index in 0..self.dirty.len() {
            if !self.dirty[index] {
                continue;
            }
            let node = NodeId::try_from_index(index).unwrap();
            let spec = &self.program.nodes()[index];
            let Some(position) = spec.position else {
                self.dirty[index] = false;
                continue;
            };
            let block = self.node_block(node);
            world.set_block(position, block);
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
            self.dirty[index] = false;
        }
        for effect in self.effects.drain(..) {
            match effect {
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

    #[cfg(test)]
    fn trace(&self) -> &[String] {
        &self.trace
    }

    #[cfg(test)]
    fn scheduled_ticks(&self) -> Vec<TickEntry> {
        self.scheduler
            .iter()
            .map(|(tick, delay, priority)| TickEntry {
                pos: tick.position,
                block_type: Some(tick.registry_type),
                ticks_left: delay as u32,
                tick_priority: priority,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;

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
