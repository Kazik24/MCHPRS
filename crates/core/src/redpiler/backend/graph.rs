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
use mchprs_blocks::blocks::{Block, ComparatorMode, RedstoneWireSide};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{TickEntry, TickPriority};
use rustc_hash::FxHashMap;
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
    UnsupportedPistonWork,
}

impl fmt::Display for GraphExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedComponent { position, kind } => {
                write!(f, "graph execution does not support {kind} at {position:?}")
            }
            Self::UnsupportedPistonWork => {
                f.write_str("graph piston actions are unavailable before piston transitions")
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
    scheduler: TickScheduler<ScheduledNodeTick>,
    pending: FxHashMap<ScheduledNodeTick, usize>,
    clock: Clock,
    dirty: Vec<bool>,
    effects: Vec<PresentationEffect>,
    #[cfg(test)]
    trace: Vec<String>,
    #[cfg(test)]
    callback_order: Vec<(BlockPos, DeliveryKind)>,
    #[cfg(test)]
    comparator_updates: Vec<BlockPos>,
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
        let mut electrical =
            electrical::prepare_electrical(world, geometry.report.bounds, &geometry.draft)
                .map_err(|error| GraphBackendError::Preparation(error.to_string()))?;
        let callbacks = callbacks::prepare_callbacks(world, &geometry.draft, &mut electrical)
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
        Ok(backend)
    }

    fn validate_supported(program: &CircuitProgram) -> Result<(), GraphBackendError> {
        for node in program.nodes() {
            let unsupported = match node.kind {
                CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => Some("command block"),
                CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => Some("note block"),
                CircuitNodeKind::Electrical(NodeType::Trapdoor) => Some("trapdoor"),
                CircuitNodeKind::Wire(_) => Some("redstone wire"),
                CircuitNodeKind::Piston(_) => Some("piston transition"),
                _ => None,
            };
            if let Some(kind) = unsupported {
                return Err(GraphBackendError::UnsupportedComponent {
                    position: node.position,
                    kind,
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
            scheduler: TickScheduler::default(),
            pending: FxHashMap::default(),
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
        Block::from_id(self.program.nodes()[node.index()].raw_state.unwrap()).registry_id()
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
        self.trace.push("phase piston_actions (empty)".into());
        self.clock.phase = GraphPhase::DeferredConnections;
        // Deferred connection events are introduced with piston transitions. This
        // explicit empty phase keeps the full-tick clock order fixed meanwhile.
        #[cfg(test)]
        self.trace.push("phase deferred_connections (empty)".into());
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
            CircuitNodeKind::Wire(_) => Err(self.unsupported(node, "redstone wire")),
            CircuitNodeKind::Piston(_) => Err(GraphExecutionError::UnsupportedPistonWork),
            CircuitNodeKind::Contact(_) => Ok(()),
        }
    }

    fn update_component(
        &mut self,
        node: NodeId,
        kind: DeliveryKind,
        captured: Option<&ObservedSite>,
    ) -> Result<(), GraphExecutionError> {
        let DeliveryKind::Update { direction } = kind else {
            return Ok(());
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
            CircuitNodeKind::Wire(_) => Err(self.unsupported(node, "redstone wire")),
            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => {
                Err(self.unsupported(node, "command block"))
            }
            CircuitNodeKind::Electrical(NodeType::NoteBlock { .. }) => {
                Err(self.unsupported(node, "note block"))
            }
            CircuitNodeKind::Electrical(NodeType::Trapdoor) => {
                Err(self.unsupported(node, "trapdoor"))
            }
            CircuitNodeKind::Piston(_) => Err(GraphExecutionError::UnsupportedPistonWork),
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
        let mut maximum = 0;
        for link in self.program.power_links() {
            if link.to != (PowerTarget::Input { node, port })
                || !self.guard_matches(link.guard, None)
            {
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
            | None => self.program.sites()[site.index()].source_strength,
        };
        match port {
            PowerPort::Output | PowerPort::Stored => stored,
            PowerPort::Weak(face) => {
                if emits_weak(resolved.block, face) {
                    stored
                } else {
                    0
                }
            }
            PowerPort::Strong(face) => {
                if emits_strong(resolved.block, face) {
                    stored
                } else {
                    0
                }
            }
            PowerPort::Conducted { include_dust } => self.conducted_power(site, include_dust),
        }
    }

    fn conducted_power(&self, site: SiteId, include_dust: bool) -> u8 {
        self.program
            .power_links()
            .iter()
            .filter(|link| {
                link.to == (PowerTarget::Conductor { site, include_dust })
                    && self.guard_matches(link.guard, None)
            })
            .map(|link| self.link_strength(link))
            .max()
            .unwrap_or(0)
    }

    fn guard_matches(&self, id: GuardId, captured: Option<&[Option<ObservedSite>]>) -> bool {
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
                    OperandView::Captured => captured
                        .and_then(|sites| sites.get(site.index()))
                        .and_then(Option::as_ref)
                        .and_then(|observed| self.resolve_observed_site(observed)),
                };
                let Some(resolved) = resolved else {
                    return false;
                };
                match test {
                    SiteTest::Solid => resolved.block.is_solid(),
                    SiteTest::Transparent => resolved.block.is_transparent(),
                    SiteTest::Cube => resolved.block.is_cube(),
                    SiteTest::IsWire => matches!(resolved.block, Block::RedstoneWire { .. }),
                    SiteTest::Profile(profile) => resolved.profile == Some(*profile),
                }
            }
            Guard::Not(child) => !self.guard_matches(*child, captured),
            Guard::All(children) => children
                .iter()
                .all(|child| self.guard_matches(*child, captured)),
            Guard::Any(children) => children
                .iter()
                .any(|child| self.guard_matches(*child, captured)),
        }
    }

    fn resolve_site(&self, site: SiteId, captured: Option<&ObservedSite>) -> Option<ResolvedSite> {
        if let Some(observed) = captured {
            return self.resolve_observed_site(observed);
        }
        let spec = *self.program.sites().get(site.index())?;
        match spec.source {
            SiteSource::Constant(profile) => {
                let block = Block::from_id(self.program.profiles()[profile.index()].raw_state);
                Some(ResolvedSite {
                    profile: Some(profile),
                    receiver: None,
                    block,
                    properties: ObservedProperties::Passive,
                })
            }
            SiteSource::Component(node) => {
                let block = self.node_block(node);
                Some(ResolvedSite {
                    profile: None,
                    receiver: Some(CallbackReceiver::Component(node)),
                    properties: self.node_properties(node),
                    block,
                })
            }
            SiteSource::Contact(contact) => {
                let spec = &self.program.contacts()[contact.index()];
                for variant in spec.variants.iter() {
                    if self.guard_matches(variant.when, None) {
                        let block = Block::from_id(
                            self.program.profiles()[variant.profile.index()].raw_state,
                        );
                        return Some(ResolvedSite {
                            profile: Some(variant.profile),
                            receiver: variant.receiver,
                            block,
                            properties: self.contact_properties(variant.receiver, block),
                        });
                    }
                }
                None
            }
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
        Some(ResolvedSite {
            profile: observed.profile,
            receiver: observed.receiver,
            block,
            properties: observed.properties.clone(),
        })
    }

    fn block_for_profile(&self, profile: ProfileId) -> Option<Block> {
        Some(Block::from_id(
            self.program.profiles().get(profile.index())?.raw_state,
        ))
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
        let mut block = Block::from_id(self.program.nodes()[node.index()].raw_state.unwrap());
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
                CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_) => {
                    return Err(GraphExecutionError::UnsupportedPistonWork);
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
