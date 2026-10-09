//! Native redstone propagation over a private snapshot; display writes are separate.
use super::{ScheduledBlockTick, TickScheduler};
use crate::redpiler::compile_graph::{CompileGraph, NodeType};
use crate::redpiler::{CompileError, TaskMonitor};
use crate::redstone;
use crate::world::{for_each_block_optimized, storage::Chunk, BlockAction, World};
use mchprs_blocks::block_entities::BlockEntity;
use mchprs_blocks::blocks::{Block, ButtonFace, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{PistonState, TickEntry, TickPriority};
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) struct NativeBackend {
    bounds: (BlockPos, BlockPos),
    blocks: FxHashMap<BlockPos, u32>,
    entities: FxHashMap<BlockPos, BlockEntity>,
    io: FxHashSet<BlockPos>,
    dirty: FxHashSet<BlockPos>,
    scheduler: TickScheduler<ScheduledBlockTick>,
    piston_state: PistonState,
    sounds: Vec<(BlockPos, i32, i32, f32, f32)>,
    actions: Vec<(BlockPos, BlockAction)>,
    commands: Vec<BlockPos>,
    dirty_commands: FxHashSet<BlockPos>,
    nodes: usize,
}

impl NativeBackend {
    pub(crate) fn compile(
        world: &impl World,
        bounds: (BlockPos, BlockPos),
        graph: &CompileGraph,
        ticks: Vec<TickEntry>,
        monitor: &TaskMonitor,
    ) -> Result<Self, CompileError> {
        super::validate_strengths(graph).map_err(CompileError::Backend)?;
        let bounds = (bounds.0.min(bounds.1), bounds.0.max(bounds.1));
        let mut backend = Self {
            bounds,
            blocks: Default::default(),
            entities: Default::default(),
            io: graph
                .node_weights()
                .filter(|node| node.is_input || node.is_output)
                .filter_map(|node| node.block.map(|(pos, _)| pos))
                .collect(),
            dirty: Default::default(),
            scheduler: ticks
                .into_iter()
                .filter(|entry| entry.pos == entry.pos.max(bounds.0).min(bounds.1))
                .collect(),
            piston_state: Default::default(),
            sounds: Vec::new(),
            actions: Vec::new(),
            commands: Vec::new(),
            dirty_commands: Default::default(),
            nodes: graph
                .node_weights()
                .filter(|node| node.ty != NodeType::Wire)
                .count(),
        };
        backend.piston_state.logical_tick = world.piston_state().logical_tick;
        // Two blocks cover solid-block power, stepped dust and far comparator inputs.
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
                        backend.blocks.insert(pos, raw);
                    }
                    if let Some(entity) = world.get_block_entity(pos) {
                        backend.entities.insert(pos, entity.clone());
                    }
                });
            }
        }
        backend.nodes += backend
            .blocks
            .iter()
            .filter(|(pos, raw)| {
                backend.owns(**pos) && matches!(Block::from_id(**raw), Block::RedstoneWire { .. })
            })
            .count();
        Ok(backend)
    }

    fn owns(&self, pos: BlockPos) -> bool {
        pos == pos.max(self.bounds.0).min(self.bounds.1)
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes
    }

    pub(crate) fn on_use_block(&mut self, pos: BlockPos) {
        let mut world = NativeWorld {
            backend: self,
            output: None,
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

    pub(crate) fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        let mut world = NativeWorld {
            backend: self,
            output: None,
        };
        if let Some(block) = world.get_block(pos).with_pressure_plate_power(powered) {
            if world.set_block(pos, block) {
                redstone::update_surrounding_blocks(&mut world, pos);
                redstone::update_surrounding_blocks(&mut world, pos.offset(BlockFace::Bottom));
            }
        }
    }

    pub(crate) fn tick(&mut self) {
        self.tick_inner(None);
    }

    pub(crate) fn tick_with_world(&mut self, world: &mut impl World) {
        world.piston_state_mut().logical_tick += 1;
        self.piston_state.logical_tick = world.piston_state().logical_tick;
        self.tick_inner(Some(world));
    }

    fn deliver_commands(&mut self, output: &mut dyn World) {
        let commands = std::mem::take(&mut self.commands);
        let mut world = NativeWorld {
            backend: self,
            output: Some(output),
        };
        for pos in commands {
            redstone::tick(world.get_block(pos), &mut world, pos);
            world.publish_commands();
        }
        world.publish_commands();
    }

    fn tick_inner<'a>(&'a mut self, output: Option<&'a mut dyn World>) {
        if output.is_none() {
            self.piston_state.logical_tick += 1;
        }
        let mut world = NativeWorld {
            backend: self,
            output,
        };
        if let Some(output) = world.output.as_deref_mut() {
            world.backend.deliver_commands(output);
        }
        for advance in [false, true] {
            if advance {
                world.backend.scheduler.end_last_tick_move_next();
            }
            while let Some(entry) = world.backend.scheduler.this_tick().pop_first() {
                let block = world.get_block(entry.pos);
                if entry
                    .block_type
                    .is_some_and(|kind| kind != block.registry_id())
                {
                    continue;
                }
                // Like the direct backend, tick() defers external command outputs to flush().
                if block.is_command_block() && world.output.is_none() {
                    world.backend.commands.push(entry.pos);
                } else {
                    redstone::tick(block, &mut world, entry.pos);
                }
                world.publish_commands();
            }
        }
        world.publish_commands();
    }

    pub(crate) fn flush(&mut self, world: &mut impl World, io_only: bool) {
        self.deliver_commands(world);
        for (pos, id, category, volume, pitch) in self.sounds.drain(..) {
            world.play_sound(pos, id, category, volume, pitch);
        }
        for (pos, action) in self.actions.drain(..) {
            world.block_action(pos, action);
        }
        let dirty = std::mem::take(&mut self.dirty);
        for pos in dirty {
            if io_only && !self.io.contains(&pos) {
                continue;
            }
            world.set_block_raw(pos, self.blocks.get(&pos).copied().unwrap_or(0));
            if let Some(entity) = self.entities.get(&pos) {
                world.set_block_entity(pos, entity.clone());
            } else if world.get_block_entity(pos).is_some() {
                world.delete_block_entity(pos);
            }
        }
    }

    pub(crate) fn reset(&mut self, world: &mut impl World, _io_only: bool) {
        self.dirty.extend(
            self.blocks
                .keys()
                .copied()
                .filter(|pos| *pos == (*pos).max(self.bounds.0).min(self.bounds.1)),
        );
        self.flush(world, false);
        for entry in self.scheduler.iter_entries() {
            world.schedule_half_tick(entry.pos, entry.ticks_left, entry.tick_priority);
        }
    }

    pub(crate) fn inspect(&mut self, pos: BlockPos) {
        tracing::debug!(?pos, block = ?Block::from_id(self.blocks.get(&pos).copied().unwrap_or(0)), "native redpiler block");
    }

    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(BlockPos, u8)> {
        self.blocks
            .iter()
            .filter_map(|(&pos, &raw)| {
                let block = Block::from_id(raw);
                if !self.owns(pos) {
                    return None;
                }
                let strength = match block {
                    Block::RedstoneComparator { .. } => match self.entities.get(&pos) {
                        Some(BlockEntity::Comparator { output_strength }) => *output_strength,
                        _ => 0,
                    },
                    Block::RedstoneTorch { lit } | Block::RedstoneWallTorch { lit, .. } => {
                        if lit {
                            15
                        } else {
                            0
                        }
                    }
                    Block::RedstoneRepeater { repeater } => {
                        if repeater.powered {
                            15
                        } else {
                            0
                        }
                    }
                    _ => return None,
                };
                Some((pos, strength))
            })
            .collect()
    }
}

struct NativeWorld<'a> {
    backend: &'a mut NativeBackend,
    output: Option<&'a mut dyn World>,
}

impl NativeWorld<'_> {
    fn publish_commands(&mut self) {
        let Some(output) = self.output.as_deref_mut() else {
            return;
        };
        // Command entities are observable outputs even before a display flush.
        for pos in std::mem::take(&mut self.backend.dirty_commands) {
            if let Some(entity) = self.backend.entities.get(&pos) {
                output.set_block_entity(pos, entity.clone());
            } else {
                output.delete_block_entity(pos);
            }
        }
    }
}

impl World for NativeWorld<'_> {
    fn get_block_raw(&self, pos: BlockPos) -> u32 {
        self.backend.blocks.get(&pos).copied().unwrap_or(0)
    }
    fn set_block_raw(&mut self, pos: BlockPos, raw: u32) -> bool {
        if !self.backend.owns(pos) || self.get_block_raw(pos) == raw {
            return false;
        }
        self.backend.blocks.insert(pos, raw);
        self.backend.dirty.insert(pos);
        true
    }
    fn get_block_entity(&self, pos: BlockPos) -> Option<&BlockEntity> {
        self.backend.entities.get(&pos)
    }
    fn get_block_entity_mut(&mut self, pos: BlockPos) -> Option<&mut BlockEntity> {
        if !self.backend.owns(pos) {
            return None;
        }
        self.backend.dirty.insert(pos);
        if matches!(
            self.backend.entities.get(&pos),
            Some(BlockEntity::CommandBlock(_))
        ) {
            self.backend.dirty_commands.insert(pos);
        }
        self.backend.entities.get_mut(&pos)
    }
    fn set_block_entity(&mut self, pos: BlockPos, entity: BlockEntity) {
        if self.backend.owns(pos) {
            if matches!(entity, BlockEntity::CommandBlock(_)) {
                self.backend.dirty_commands.insert(pos);
            }
            self.backend.entities.insert(pos, entity);
            self.backend.dirty.insert(pos);
        }
    }
    fn delete_block_entity(&mut self, pos: BlockPos) {
        if self.backend.owns(pos) {
            if matches!(
                self.backend.entities.get(&pos),
                Some(BlockEntity::CommandBlock(_))
            ) {
                self.backend.dirty_commands.insert(pos);
            }
            self.backend.entities.remove(&pos);
            self.backend.dirty.insert(pos);
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
        if self.backend.owns(pos) && !self.backend.scheduler.contains(&entry) {
            self.backend
                .scheduler
                .schedule_half_tick(entry, delay as usize, priority);
        }
    }
    fn pending_tick_at(&mut self, pos: BlockPos) -> bool {
        self.backend.scheduler.contains(&ScheduledBlockTick {
            pos,
            block_type: Some(self.get_block(pos).registry_id()),
        })
    }
    fn block_action(&mut self, pos: BlockPos, action: BlockAction) {
        self.backend.actions.push((pos, action));
    }
    fn play_sound(&mut self, pos: BlockPos, id: i32, category: i32, volume: f32, pitch: f32) {
        self.backend.sounds.push((pos, id, category, volume, pitch));
    }
    fn execute_command_block(&mut self, command: &str, source: &str) -> Result<(), String> {
        self.output
            .as_deref_mut()
            .expect("command callback requires an output world")
            .execute_command_block(command, source)
    }
}
