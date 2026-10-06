//! Physical compatibility execution for update-sensitive BUD networks.
//!
//! The shared interpreter owns every callback and deadline in a private plot.
//! There is deliberately no electrical/physical scheduler boundary: until that
//! boundary has an ordering proof, dividing a CPU would change its BUD writes.
//! Presentation publishes I/O; reset transfers the actual physical state rather
//! than reconstructing it from a logical count or replaying its entire history.

use super::{BackendError, JITBackend};
use crate::plot::{PlotWorld, PLOT_BLOCK_HEIGHT, PLOT_BLOCK_WIDTH, PLOT_WIDTH};
use crate::redpiler::compile_graph::CompileGraph;
use crate::redpiler::{CompilerOptions, TaskMonitor};
use crate::redstone;
use crate::world::{for_each_block_optimized, World};
use mchprs_blocks::blocks::{Block, ButtonFace, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use mchprs_world::{AdvancePhase, TickEntry, TickPriority};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use tracing::{debug, warn};

pub struct EventBackend {
    world: PlotWorld,
    modified: FxHashSet<BlockPos>,
    visible: FxHashSet<BlockPos>,
    pending_visual: FxHashMap<BlockPos, u32>,
    changes: Vec<(BlockPos, u32)>,
}

impl EventBackend {
    pub(crate) fn prepare(
        source: &impl World,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: &TaskMonitor,
    ) -> Result<Self, String> {
        if options.optimize || options.export || options.export_dot_graph || options.update {
            return Err("piston event mode currently supports I/O presentation only; remove --optimize, --export, --export-dot and --update".into());
        }
        if source.piston_state().phase != AdvancePhase::BetweenTicks {
            return Err("piston event mode requires a completed game-tick boundary".into());
        }
        if source.is_cursed() {
            return Err("piston event mode requires standard support checks".into());
        }
        if !source.supports_exact_tick_transfer() {
            return Err(
                "piston event mode requires a world with exact scheduled-work transfer".into(),
            );
        }
        let first = bounds.0.min(bounds.1);
        let last = bounds.0.max(bounds.1);
        let plot_x = first.x.div_euclid(PLOT_BLOCK_WIDTH);
        let plot_z = first.z.div_euclid(PLOT_BLOCK_WIDTH);
        if first != BlockPos::new(plot_x * PLOT_BLOCK_WIDTH, 0, plot_z * PLOT_BLOCK_WIDTH)
            || last
                != first
                    + BlockPos::new(
                        PLOT_BLOCK_WIDTH - 1,
                        PLOT_BLOCK_HEIGHT - 1,
                        PLOT_BLOCK_WIDTH - 1,
                    )
        {
            return Err("piston event mode requires one complete plot so update paths have a single execution owner".into());
        }
        if ticks.iter().any(|tick| {
            tick.pos.x < first.x
                || tick.pos.x > last.x
                || tick.pos.y < first.y
                || tick.pos.y > last.y
                || tick.pos.z < first.z
                || tick.pos.z > last.z
                || tick.ticks_left >= 32
        }) {
            return Err(
                "piston event mode received scheduled work outside its plot or scheduler window"
                    .into(),
            );
        }
        monitor.set_message("Preparing ordered piston/BUD compatibility execution".into());
        monitor.set_max_progress((PLOT_WIDTH * PLOT_WIDTH) as usize);
        let mut chunks = Vec::with_capacity((PLOT_WIDTH * PLOT_WIDTH) as usize);
        for x in plot_x * PLOT_WIDTH..(plot_x + 1) * PLOT_WIDTH {
            for z in plot_z * PLOT_WIDTH..(plot_z + 1) * PLOT_WIDTH {
                if monitor.cancelled() {
                    return Err("piston event compilation cancelled".into());
                }
                chunks.push(
                    source
                        .get_chunk(x, z)
                        .ok_or_else(|| format!("piston event mode requires loaded chunk {x}, {z}"))?
                        .snapshot(),
                );
                monitor.inc_progress();
            }
        }
        let mut world = PlotWorld::from_chunks(plot_x, plot_z, chunks, Default::default());
        world.restore_tick_requests(ticks);
        *world.piston_state_mut() = source.piston_state().clone();
        let mut visible = FxHashSet::default();
        let mut command = None;
        for_each_block_optimized(&world, first, last, |pos| {
            let block = world.get_block(pos);
            let projected = Block::from_id(world.screen_state(pos));
            if block.is_command_block() || projected.is_command_block() {
                command.get_or_insert(pos);
            }
            if is_visible(projected) {
                visible.insert(pos);
            }
        });
        if let Some(pos) = command {
            return Err(format!(
                "command block at {pos:?} needs an output adapter before piston event compilation"
            ));
        }
        if monitor.cancelled() {
            return Err("piston event compilation cancelled".into());
        }
        Ok(Self {
            world,
            modified: Default::default(),
            visible,
            pending_visual: Default::default(),
            changes: Vec::new(),
        })
    }

    fn collect_changes(&mut self) {
        self.world.collect_block_changes(&mut self.changes);
        for (pos, raw) in self.changes.drain(..) {
            self.modified.insert(pos);
            let state = if matches!(Block::from_id(raw), Block::MovingPiston { .. }) {
                self.world.screen_state(pos)
            } else {
                raw
            };
            if is_visible(Block::from_id(state)) {
                self.visible.insert(pos);
            }
            if self.visible.contains(&pos) {
                self.pending_visual.insert(pos, state);
            }
        }
    }

    fn notify_control(&mut self, pos: BlockPos, support: BlockFace) {
        redstone::update_surrounding_blocks(&mut self.world, pos);
        redstone::update_surrounding_blocks(&mut self.world, pos.offset(support));
        self.collect_changes();
    }
}

fn is_visible(block: Block) -> bool {
    matches!(
        block,
        Block::RedstoneLamp { .. }
            | Block::Lever { .. }
            | Block::StoneButton { .. }
            | Block::IronTrapdoor { .. }
            | Block::NoteBlock { .. }
    ) || block.pressure_plate_powered().is_some()
}

impl JITBackend for EventBackend {
    fn compile(
        &mut self,
        _: CompileGraph,
        _: Vec<TickEntry>,
        _: &CompilerOptions,
        _: Arc<TaskMonitor>,
    ) -> Result<(), BackendError> {
        Err(BackendError::EventProgramRequired)
    }

    fn tick(&mut self) {
        self.world.tick_interpreted();
        self.collect_changes();
    }

    fn on_use_block(&mut self, pos: BlockPos) {
        match self.world.get_block(pos) {
            Block::Lever { mut lever } => {
                lever.powered = !lever.powered;
                self.world.set_block(pos, Block::Lever { lever });
                let support = match lever.face {
                    LeverFace::Floor => BlockFace::Bottom,
                    LeverFace::Ceiling => BlockFace::Top,
                    LeverFace::Wall => lever.facing.opposite().block_face(),
                };
                self.notify_control(pos, support);
            }
            Block::StoneButton { mut button } if !button.powered => {
                button.powered = true;
                self.world.set_block(pos, Block::StoneButton { button });
                self.world.schedule_tick(pos, 10, TickPriority::Normal);
                let support = match button.face {
                    ButtonFace::Floor => BlockFace::Bottom,
                    ButtonFace::Ceiling => BlockFace::Top,
                    ButtonFace::Wall => button.facing.opposite().block_face(),
                };
                self.notify_control(pos, support);
            }
            Block::StoneButton { .. } => {}
            block => warn!(
                "Tried to use a {} piston event input at {pos:?}",
                block.get_name()
            ),
        }
    }

    fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        if let Some(block) = self.world.get_block(pos).with_pressure_plate_power(powered) {
            // The physical interaction sends notifications even when the
            // requested strength is unchanged. A BUD can sample that update.
            self.world.set_block(pos, block);
            redstone::update_surrounding_blocks(&mut self.world, pos);
            redstone::update_surrounding_blocks(&mut self.world, pos.offset(BlockFace::Bottom));
            self.collect_changes();
        }
    }

    fn flush<W: World>(&mut self, world: &mut W, _: bool) {
        for (pos, state) in self.pending_visual.drain() {
            world.set_block_raw(pos, state);
        }
        self.world.flush_generated_sounds(world);
    }

    fn reset<W: World>(&mut self, world: &mut W, io_only: bool) {
        self.flush(world, io_only);
        for &pos in &self.modified {
            world.set_block_raw(pos, self.world.get_block_raw(pos));
            world.delete_block_entity(pos);
        }
        // Entity-only changes (comparator output and motion progress) need no
        // block-state delta. Transfer every live entity after writing geometry.
        for chunk in self.world.get_chunks() {
            for (local, entity) in &chunk.block_entities {
                let pos = BlockPos::new(chunk.x * 16 + local.x, local.y, chunk.z * 16 + local.z);
                world.set_block_entity(pos, entity.clone());
            }
        }
        *world.piston_state_mut() = self.world.piston_state().clone();
        world.restore_tick_requests(self.world.scheduler().iter_entries().collect());
        self.modified.clear();
    }

    fn inspect(&mut self, pos: BlockPos) {
        debug!(block = ?self.world.get_block(pos), entity = ?self.world.get_block_entity(pos), tick = self.world.piston_state().logical_tick, "Piston event compatibility state at {pos:?}");
    }
}

#[cfg(test)]
mod tests;
