//! Ordinary physical propagation; clock, scheduling and assemblies belong to the runtime.
use super::{direct::DirectBackend, RuntimeTick, ScheduledBlockTick};
use crate::redpiler::compile_graph::CompileGraph;
use crate::redpiler::{CompileError, TaskMonitor};
use crate::redstone;
use crate::world::{for_each_block_optimized, storage::Chunk, BlockAction, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ButtonFace, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{PistonState, TickPriority};
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) struct NativePropagation {
    owned: FxHashSet<BlockPos>,
    restored: FxHashSet<BlockPos>,
    composed: bool,
    blocks: FxHashMap<BlockPos, u32>,
    entities: FxHashMap<BlockPos, BlockEntity>,
    io: FxHashSet<BlockPos>,
    dirty: FxHashSet<BlockPos>,
    sounds: Vec<(BlockPos, i32, i32, f32, f32)>,
    actions: Vec<(BlockPos, BlockAction)>,
    commands: Vec<BlockPos>,
    dirty_commands: FxHashSet<BlockPos>,
}

impl NativePropagation {
    pub(crate) fn compile(
        world: &impl World,
        bounds: (BlockPos, BlockPos),
        graph: &CompileGraph,
        monitor: &TaskMonitor,
    ) -> Result<Self, CompileError> {
        super::validate_strengths(graph).map_err(CompileError::Backend)?;
        let bounds = (bounds.0.min(bounds.1), bounds.0.max(bounds.1));
        let mut propagation = Self {
            owned: graph
                .node_weights()
                .filter(|node| node.native)
                .filter_map(|node| node.block.map(|(pos, _)| pos))
                .collect(),
            restored: graph
                .node_weights()
                .filter(|node| {
                    node.native && node.ty != crate::redpiler::compile_graph::NodeType::Constant
                })
                .filter_map(|node| node.block.map(|(pos, _)| pos))
                .collect(),
            composed: false,
            io: graph
                .node_weights()
                .filter(|node| node.native && (node.is_input || node.is_output))
                .filter_map(|node| node.block.map(|(pos, _)| pos))
                .collect(),
            blocks: Default::default(),
            entities: Default::default(),
            dirty: Default::default(),
            sounds: Vec::new(),
            actions: Vec::new(),
            commands: Vec::new(),
            dirty_commands: Default::default(),
        };
        let first = bounds.0 - BlockPos::new(2, 2, 2);
        let last = bounds.1 + BlockPos::new(2, 2, 2);
        for z in first.z.div_euclid(16)..=last.z.div_euclid(16) {
            for x in first.x.div_euclid(16)..=last.x.div_euclid(16) {
                if monitor.cancelled() {
                    return Err(CompileError::Cancelled);
                }
                if world.get_chunk(x, z).is_none() {
                    continue;
                }
                let low = first.max(BlockPos::new(x * 16, 0, z * 16));
                let high = last.min(BlockPos::new(
                    x * 16 + 15,
                    crate::plot::PLOT_BLOCK_HEIGHT - 1,
                    z * 16 + 15,
                ));
                for_each_block_optimized(world, low, high, |pos| {
                    let raw = world.get_block_raw(pos);
                    if raw != 0 {
                        propagation.blocks.insert(pos, raw);
                    }
                    if let Some(entity) = world.get_block_entity(pos) {
                        propagation.entities.insert(pos, entity.clone());
                    }
                });
            }
        }
        Ok(propagation)
    }
    pub(crate) fn owns(&self, pos: BlockPos) -> bool {
        self.owned.contains(&pos)
    }
    pub(crate) fn near(&self, pos: BlockPos) -> bool {
        (-2i32..=2).any(|x| {
            (-2i32..=2).any(|y| {
                (-2i32..=2).any(|z| {
                    x.abs() + y.abs() + z.abs() <= 2 && self.owns(pos + BlockPos::new(x, y, z))
                })
            })
        })
    }
    pub(crate) fn bind_boundary(&mut self, owned: &FxHashSet<BlockPos>) {
        self.composed |= owned.iter().any(|&pos| self.near(pos));
    }
}

impl DirectBackend {
    pub(super) fn native_geometry_ports(&mut self, pos: BlockPos) {
        if self.native.is_none() {
            return;
        }
        let mut world = NativeWorld {
            backend: self,
            output: None,
            resolving_ports: true,
            geometry_port_capture: true,
        };
        world.refresh_ports(pos);
        world.backend.begin_native_callback_batch();
        redstone::piston::notify(&mut world, pos);
        world.backend.end_native_callback_batch();
        world.backend.finish_native_delivery();
    }
    pub(super) fn native_update(&mut self, pos: BlockPos) {
        let mut world = NativeWorld {
            backend: self,
            output: None,
            resolving_ports: true,
            geometry_port_capture: false,
        };
        world.backend.begin_native_callback_batch();
        redstone::update(world.get_block(pos), &mut world, pos, None);
        world.backend.end_native_callback_batch();
    }
    pub(crate) fn attach_native(
        &mut self,
        world: &impl World,
        bounds: (BlockPos, BlockPos),
        graph: &CompileGraph,
        monitor: &TaskMonitor,
    ) -> Result<(), CompileError> {
        self.piston_state.logical_tick = world.piston_state().logical_tick;
        if graph.node_weights().any(|node| node.native) {
            self.native = Some(NativePropagation::compile(world, bounds, graph, monitor)?);
        }
        Ok(())
    }
    pub(super) fn native_on_use(&mut self, pos: BlockPos) {
        let mut world = NativeWorld {
            backend: self,
            output: None,
            resolving_ports: true,
            geometry_port_capture: false,
        };
        let support = match world.get_block(pos) {
            Block::Lever { mut lever } => {
                lever.powered = !lever.powered;
                world.set_block(pos, Block::Lever { lever });
                match lever.face {
                    LeverFace::Floor => BlockFace::Bottom,
                    LeverFace::Ceiling => BlockFace::Top,
                    LeverFace::Wall => lever.facing.opposite().block_face(),
                }
            }
            Block::StoneButton { mut button } if !button.powered => {
                button.powered = true;
                world.set_block(pos, Block::StoneButton { button });
                world.schedule_tick(pos, 10, TickPriority::Normal);
                match button.face {
                    ButtonFace::Floor => BlockFace::Bottom,
                    ButtonFace::Ceiling => BlockFace::Top,
                    ButtonFace::Wall => button.facing.opposite().block_face(),
                }
            }
            _ => return,
        };
        redstone::update_surrounding_blocks(&mut world, pos);
        redstone::update_surrounding_blocks(&mut world, pos.offset(support));
    }
    pub(super) fn native_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        let mut world = NativeWorld {
            backend: self,
            output: None,
            resolving_ports: true,
            geometry_port_capture: false,
        };
        if let Some(block) = world.get_block(pos).with_pressure_plate_power(powered) {
            if world.set_block(pos, block) {
                redstone::update_surrounding_blocks(&mut world, pos);
                redstone::update_surrounding_blocks(&mut world, pos.offset(BlockFace::Bottom));
            }
        }
    }
    pub(super) fn native_tick(
        &mut self,
        entry: ScheduledBlockTick,
        output: Option<&mut dyn World>,
    ) {
        let mut world = NativeWorld {
            backend: self,
            output,
            resolving_ports: true,
            geometry_port_capture: false,
        };
        let block = world.get_block(entry.pos);
        if entry
            .block_type
            .is_some_and(|kind| kind != block.registry_id())
        {
            return;
        }
        if block.is_command_block() && world.output.is_none() {
            world
                .backend
                .native
                .as_mut()
                .unwrap()
                .commands
                .push(entry.pos);
        } else {
            redstone::tick(block, &mut world, entry.pos);
        }
        world.publish_commands();
        world.backend.finish_native_delivery();
        world.backend.evaluate_instant(false);
    }
    pub(super) fn native_commands(&mut self, output: &mut dyn World) {
        let Some(native) = &mut self.native else {
            return;
        };
        let commands = std::mem::take(&mut native.commands);
        for pos in commands {
            self.native_tick(
                ScheduledBlockTick {
                    pos,
                    block_type: None,
                },
                Some(output),
            );
        }
        NativeWorld {
            backend: self,
            output: Some(output),
            resolving_ports: true,
            geometry_port_capture: false,
        }
        .publish_commands();
    }
    pub(super) fn flush_native(&mut self, world: &mut impl World, io_only: bool) {
        self.native_commands(world);
        let Some(native) = &mut self.native else {
            return;
        };
        for (pos, id, category, volume, pitch) in native.sounds.drain(..) {
            world.play_sound(pos, id, category, volume, pitch);
        }
        for (pos, action) in native.actions.drain(..) {
            world.block_action(pos, action);
        }
        for pos in std::mem::take(&mut native.dirty) {
            if io_only && !native.io.contains(&pos) {
                continue;
            }
            world.set_block_raw(pos, native.blocks.get(&pos).copied().unwrap_or(0));
            if let Some(entity) = native.entities.get(&pos) {
                world.set_block_entity(pos, entity.clone());
            } else if world.get_block_entity(pos).is_some() {
                world.delete_block_entity(pos);
            }
        }
    }
    pub(super) fn restore_native(&mut self, world: &mut impl World) {
        if let Some(native) = &mut self.native {
            native.dirty.extend(native.restored.iter().copied());
        }
        self.flush_native(world, false);
    }
}
struct NativeWorld<'a, 'b> {
    backend: &'a mut DirectBackend,
    output: Option<&'b mut dyn World>,
    resolving_ports: bool,
    geometry_port_capture: bool,
}

impl NativeWorld<'_, '_> {
    fn refresh_ports(&mut self, changed: BlockPos) {
        let Some(indices) = self
            .backend
            .native_ports_affected_by(changed)
            .map(|indices| indices.to_vec())
        else {
            return;
        };
        let bindings: Vec<_> = indices
            .into_iter()
            .map(|index| self.backend.native_port_bindings()[index])
            .collect();
        self.resolving_ports = false;
        let values = bindings
            .into_iter()
            .map(|(pos, side, id)| (id, redstone::consumer_input(self, pos, side)))
            .collect();
        self.resolving_ports = true;
        self.backend.commit_native_ports(values);
    }
    fn record(&mut self, pos: BlockPos) {
        let block = self.get_block(pos);
        let strength = redstone::source_strength(block, self, pos);
        self.backend.record_native(pos, block, strength);
        self.refresh_ports(pos);
    }
    fn publish_commands(&mut self) {
        let dirty: Vec<_> = self
            .backend
            .native
            .as_ref()
            .unwrap()
            .dirty_commands
            .iter()
            .copied()
            .collect();
        for pos in dirty {
            self.record(pos);
        }
        let Some(output) = self.output.as_deref_mut() else {
            return;
        };
        // Command entities are observable outputs even before a display flush.
        for pos in std::mem::take(&mut self.backend.native.as_mut().unwrap().dirty_commands) {
            if let Some(entity) = self.backend.native.as_ref().unwrap().entities.get(&pos) {
                output.set_block_entity(pos, entity.clone());
            } else {
                output.delete_block_entity(pos);
            }
        }
    }
}

impl World for NativeWorld<'_, '_> {
    fn owns_redstone_update(&self, pos: BlockPos) -> bool {
        self.backend.assembly_owners.contains_key(&pos)
    }
    fn dispatch_neighbor_shape_update(&mut self, pos: BlockPos, _: BlockFace) -> bool {
        self.backend.assembly_owners.contains_key(&pos)
            || (self.geometry_port_capture
                && !matches!(self.get_block(pos), Block::RedstoneWire { .. }))
    }
    fn resolved_redstone_input(&self, pos: BlockPos, side: bool) -> Option<u8> {
        self.resolving_ports
            .then(|| self.backend.resolved_input(pos, side))
            .flatten()
    }
    fn dispatch_redstone_update(
        &mut self,
        pos: BlockPos,
        dir: Option<BlockFace>,
        source: Option<BlockPos>,
    ) -> bool {
        if self.geometry_port_capture && matches!(self.get_block(pos), Block::Observer { .. }) {
            return true;
        }
        if !self.backend.native.as_ref().unwrap().composed
            && self.backend.native.as_ref().unwrap().owns(pos)
        {
            return false;
        }
        self.refresh_ports(pos);
        self.backend.dispatch_owned_update(pos, dir, source)
    }
    fn get_block_raw(&self, pos: BlockPos) -> u32 {
        let native = self.backend.native.as_ref().unwrap();
        if self.geometry_port_capture {
            if let Some(block) = self.backend.geometry_port_block_at(pos) {
                return block.get_id();
            }
        }
        native.blocks.get(&pos).copied().unwrap_or(0)
    }
    fn set_block_raw(&mut self, pos: BlockPos, raw: u32) -> bool {
        if !self.backend.native.as_ref().unwrap().owns(pos) || self.get_block_raw(pos) == raw {
            return false;
        }
        self.backend
            .native
            .as_mut()
            .unwrap()
            .blocks
            .insert(pos, raw);
        self.backend.native.as_mut().unwrap().dirty.insert(pos);
        self.record(pos);
        true
    }
    fn get_block_entity(&self, pos: BlockPos) -> Option<&BlockEntity> {
        self.backend.native.as_ref().unwrap().entities.get(&pos)
    }
    fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut BlockEntity> {
        if !self.backend.native.as_ref().unwrap().owns(pos) {
            return None;
        }
        self.backend.native.as_mut().unwrap().dirty.insert(pos);
        if matches!(
            self.backend.native.as_ref().unwrap().entities.get(&pos),
            Some(BlockEntity::CommandBlock(_))
        ) {
            self.backend
                .native
                .as_mut()
                .unwrap()
                .dirty_commands
                .insert(pos);
        }
        self.backend.native.as_mut().unwrap().entities.get_mut(&pos)
    }
    fn set_block_entity(&mut self, pos: BlockPos, entity: BlockEntity) {
        if self.backend.native.as_ref().unwrap().owns(pos) {
            if matches!(entity, BlockEntity::CommandBlock(_)) {
                self.backend
                    .native
                    .as_mut()
                    .unwrap()
                    .dirty_commands
                    .insert(pos);
            }
            self.backend
                .native
                .as_mut()
                .unwrap()
                .entities
                .insert(pos, entity);
            self.backend.native.as_mut().unwrap().dirty.insert(pos);
            self.record(pos);
        }
    }
    fn delete_block_entity(&mut self, pos: BlockPos) {
        if self.backend.native.as_ref().unwrap().owns(pos) {
            if matches!(
                self.backend.native.as_ref().unwrap().entities.get(&pos),
                Some(BlockEntity::CommandBlock(_))
            ) {
                self.backend
                    .native
                    .as_mut()
                    .unwrap()
                    .dirty_commands
                    .insert(pos);
            }
            self.backend.native.as_mut().unwrap().entities.remove(&pos);
            self.backend.native.as_mut().unwrap().dirty.insert(pos);
        }
    }
    fn piston_state(&self) -> &PistonState {
        &self.backend.piston_state
    }
    fn piston_state_mut(&mut self) -> &mut PistonState {
        &mut self.backend.piston_state
    }
    // Ordinary native redstone uses block reads; this sparse snapshot has no chunks.
    fn get_chunk(&self, _: i32, _: i32) -> Option<&Chunk> {
        None
    }
    fn get_chunk_mut(&mut self, _: i32, _: i32) -> Option<&mut Chunk> {
        None
    }
    fn schedule_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        self.schedule_half_tick(pos, delay * 2, priority);
    }
    fn schedule_half_tick(&mut self, pos: BlockPos, delay: u32, priority: TickPriority) {
        let entry = ScheduledBlockTick {
            pos,
            block_type: Some(self.get_block(pos).registry_id()),
        };
        if self.backend.native.as_ref().unwrap().owns(pos)
            && !self.backend.scheduler.contains(&RuntimeTick::Block(entry))
        {
            self.backend.scheduler.schedule_half_tick(
                RuntimeTick::Block(entry),
                delay as usize,
                priority,
            );
        }
    }
    fn pending_tick_at(&mut self, pos: BlockPos) -> bool {
        self.backend
            .scheduler
            .contains(&RuntimeTick::Block(ScheduledBlockTick {
                pos,
                block_type: Some(self.get_block(pos).registry_id()),
            }))
    }
    fn block_action(&mut self, pos: BlockPos, action: BlockAction) {
        self.backend
            .native
            .as_mut()
            .unwrap()
            .actions
            .push((pos, action));
    }
    fn play_sound(&mut self, pos: BlockPos, id: i32, category: i32, volume: f32, pitch: f32) {
        self.backend
            .native
            .as_mut()
            .unwrap()
            .sounds
            .push((pos, id, category, volume, pitch));
    }
    fn execute_command_block(&mut self, command: &str, source: &str) -> Result<(), String> {
        self.output
            .as_deref_mut()
            .expect("command callback requires an output world")
            .execute_command_block(command, source)
    }
}
