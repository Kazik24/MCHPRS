//! Sampled compiled redstone execution.

mod options;
mod queue;
mod sampled;
mod scratch;
mod task_monitor;
#[cfg(test)]
mod tests;

use crate::redstone;
use crate::world::{World, for_each_block_mut_optimized};
use mchprs_blocks::{BlockPos, block_entities::BlockEntity, blocks::Block};
use mchprs_world::{AdvancePhase, PistonMotion, TickEntry};
use sampled::{ExportKey, Runtime, ScratchEntry, ScratchError};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use options::CompilerOptions;
pub use queue::{Queues, ScheduledBlockTick, TickScheduler};
pub use task_monitor::TaskMonitor;

#[derive(Debug)]
pub enum CompileError {
    AlreadyActive,
    Unsupported(ScratchError),
    Cancelled,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActive => f.write_str("reset the active compiler before recompiling"),
            Self::Unsupported(error) => {
                if let Some(position) = error.position {
                    write!(f, "unsupported circuit at {position:?}: {}", error.reason)
                } else {
                    f.write_str(error.reason)
                }
            }
            Self::Cancelled => f.write_str("compilation cancelled"),
        }
    }
}

impl std::error::Error for CompileError {}

#[derive(Debug, Clone)]
pub struct ExecutionError(ScratchError);

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "compiled runtime at {:?}: {}", self.0.position, self.0.reason)
    }
}

impl std::error::Error for ExecutionError {}

#[derive(Debug, Clone)]
pub struct CompileStatistics {
    pub components: usize,
    pub pending_ticks: usize,
    pub duration: Duration,
}

impl CompileStatistics {
    pub fn chat_summary_lines(&self) -> Vec<String> {
        vec![format!("Sampled compiled {} components in {:?}", self.components, self.duration)]
    }
}

#[derive(Default)]
pub struct Compiler {
    runtime: Option<Runtime>,
    options: CompilerOptions,
    statistics: Option<CompileStatistics>,
    exported_motions: HashMap<ExportKey, u64>,
}

impl Compiler {
    pub fn stats(&self) -> Option<&CompileStatistics> {
        self.statistics.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.runtime.is_some()
    }

    pub fn current_flags(&self) -> Option<&CompilerOptions> {
        self.runtime.as_ref().map(|_| &self.options)
    }

    pub fn compile<W: World>(
        &mut self,
        world: &W,
        bounds: (BlockPos, BlockPos),
        options: CompilerOptions,
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
    ) -> Result<(), CompileError> {
        if self.is_active() {
            return Err(CompileError::AlreadyActive);
        }
        let start = Instant::now();
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        let entry = ScratchEntry {
            scheduled: ticks.clone(),
            piston: world.piston_state().clone(),
        };
        let program = sampled::compile_program(world, bounds, entry, Some(&monitor)).map_err(|error| {
            if monitor.cancelled() {
                CompileError::Cancelled
            } else {
                CompileError::Unsupported(error)
            }
        })?;
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        let components = program.component_count();
        let pending_ticks = ticks.len();
        let runtime = sampled::start_runtime(program, ticks, world.piston_state().clone());
        self.runtime = Some(runtime);
        self.exported_motions.clear();
        self.statistics = Some(CompileStatistics {
            components,
            pending_ticks,
            duration: start.elapsed(),
        });
        self.options = options;
        Ok(())
    }

    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        if let Err(error) = self.try_reset(world, bounds) {
            tracing::error!(%error, "compiled reset failed; compiler state retained");
        }
    }

    pub fn try_reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) -> Result<(), ExecutionError> {
        if self.runtime.is_some() {
            self.publish(world, false)?;
            let runtime = self.runtime.as_ref().unwrap();
            let pending = runtime.pending_entries();
            self.runtime = None;
            for entry in pending {
                world.restore_scheduled_tick(entry);
            }
            self.exported_motions.clear();
        }
        if self.options.update {
            let (first_pos, second_pos) = bounds;
            for_each_block_mut_optimized(world, first_pos, second_pos, |world, pos| {
                let block = world.get_block(pos);
                redstone::update(block, world, pos, None);
            });
        }
        self.options = Default::default();
        Ok(())
    }

    pub fn tick(&mut self) -> Result<(), ExecutionError> {
        self.runtime.as_mut().expect("compiler inactive").tick().map_err(ExecutionError)
    }

    pub fn tick_with_world<W: World>(&mut self, _world: &mut W) -> Result<(), ExecutionError> {
        self.tick()
    }

    pub fn on_use_block(&mut self, pos: BlockPos) {
        if let Err(error) = self.try_on_use_block(pos) {
            tracing::error!(%error, "sampled runtime control failed");
        }
    }

    pub fn try_on_use_block(&mut self, pos: BlockPos) -> Result<(), ExecutionError> {
        self.runtime.as_mut().expect("compiler inactive").use_control(pos).map_err(ExecutionError)
    }

    pub(crate) fn accepts_control(&self, pos: BlockPos) -> bool {
        self.runtime.as_ref().is_some_and(|runtime| runtime.accepts_control(pos))
    }

    pub fn flush<W: World>(&mut self, world: &mut W) {
        if let Err(error) = self.try_flush(world) {
            tracing::error!(%error, "sampled state publication failed");
        }
    }

    pub fn try_flush<W: World>(&mut self, world: &mut W) -> Result<(), ExecutionError> {
        self.publish(world, self.options.io_only)
    }

    fn publish<W: World>(&mut self, world: &mut W, io_only: bool) -> Result<(), ExecutionError> {
        let runtime = self.runtime.as_ref().expect("compiler inactive");
        let parts = if io_only {
            Vec::new()
        } else {
            runtime.moving_parts().map_err(ExecutionError)?
        };
        let positions: HashSet<_> = parts.iter().map(|part| part.pos).collect();
        if positions.len() != parts.len() {
            return Err(ExecutionError(ScratchError {
                position: None,
                reason: "overlapping piston motion parts cannot be published",
            }));
        }
        let old = self.exported_motions.clone();
        for (key, _) in &old {
            world.delete_block_entity(key.3);
        }
        for (pos, block) in runtime.projected_blocks() {
            if io_only && !is_output(block) {
                continue;
            }
            if positions.contains(&pos) {
                continue;
            }
            if world.get_block(pos) != block {
                world.set_block(pos, block);
            }
        }
        if !io_only {
            for (pos, output_strength) in runtime.comparator_outputs() {
                if let Some(BlockEntity::Comparator { output_strength: current }) = world.get_block_entity_mut(pos) {
                    *current = output_strength;
                }
            }
        }
        if io_only {
            return Ok(());
        }
        let old_ids: HashSet<_> = old.values().copied().collect();
        let runtime = self.runtime.as_ref().unwrap();
        let piston_state = runtime.piston_state();
        let logical_tick = piston_state.logical_tick.saturating_add(runtime.motion_time());
        let pending_events = runtime.pending_piston_events();
        let old_carried: HashMap<_, _> = old
            .iter()
            .filter_map(|(key, identity)| {
                world
                    .piston_state()
                    .motions
                    .iter()
                    .find(|motion| motion.identity == *identity)
                    .map(|motion| (*key, motion.carried_entity.clone()))
            })
            .collect();
        let mut next_identity = world.piston_state().next_identity;
        let mut next_export = HashMap::new();
        let mut motions = Vec::with_capacity(parts.len());
        for part in &parts {
            let identity = old.get(&part.key).copied().unwrap_or_else(|| {
                next_identity = next_identity.saturating_add(1);
                next_identity
            });
            next_export.insert(part.key, identity);
            motions.push(PistonMotion {
                pos: part.pos,
                identity,
                progress: part.progress,
                previous_progress: (part.progress - 0.5).max(0.0),
                last_tick: logical_tick,
                carried_entity: old_carried.get(&part.key).cloned().flatten(),
            });
        }
        {
            let state = world.piston_state_mut();
            state.motions.retain(|motion| !old_ids.contains(&motion.identity));
            state.logical_tick = logical_tick;
            state.phase = AdvancePhase::BetweenTicks;
            state.scheduled_advanced = false;
            state.events = pending_events.into();
            state.movement_work.clear();
            state.movement_cursor = 0;
            state.next_identity = next_identity;
        }
        for part in &parts {
            world.delete_block_entity(part.pos);
            world.set_block(part.pos, part.block);
            world.set_block_entity(part.pos, BlockEntity::MovingPiston(part.entity));
        }
        {
            let state = world.piston_state_mut();
            state.motions.retain(|motion| !positions.contains(&motion.pos));
            state.motions.extend(motions);
        }
        self.exported_motions = next_export;
        Ok(())
    }
}

fn is_output(block: Block) -> bool {
    matches!(block, Block::Lever { .. } | Block::RedstoneLamp { .. })
}
