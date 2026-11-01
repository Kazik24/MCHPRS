//! Sampled compiled redstone execution.

#[cfg(test)]
mod classic_fuzz_tests;
mod options;
#[cfg(test)]
mod publication_tests;
mod sampled;
mod scratch;
mod task_monitor;
#[cfg(test)]
mod tests;

use crate::redstone;
use crate::world::{World, for_each_block_mut_optimized};
use mchprs_blocks::{BlockPos, block_entities::BlockEntity, blocks::Block};
use mchprs_world::{AdvancePhase, PistonMotion, TickEntry};
use sampled::{ExportKey, MovingPart, Runtime, ScratchEntry, ScratchError};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use crate::world::scheduler::{Queues, ScheduledBlockTick, TickScheduler};
pub use options::CompilerOptions;
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
pub struct ExecutionError(
    /// underlying runtime failure, including its optional world position and reason.
    ScratchError,
);

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "compiled runtime at {:?}: {}", self.0.position, self.0.reason)
    }
}

impl std::error::Error for ExecutionError {}

#[derive(Debug, Clone)]
pub struct CompileStatistics {
    /// number of compiled redstone components.
    pub components: usize,
    /// number of native scheduled ticks copied into the compiled runtime.
    pub pending_ticks: usize,
    /// elapsed compile time.
    pub duration: Duration,
}

impl CompileStatistics {
    pub fn chat_summary_lines(&self) -> Vec<String> {
        vec![format!("Sampled compiled {} components in {:?}", self.components, self.duration)]
    }
}

struct ExportedMotion {
    /// host motion identity used to correlate updates across publications.
    identity: u64,
    /// whether this host motion was observed in the current publication pass.
    seen: bool,
}

#[derive(Default)]
pub struct Compiler {
    /// active compiled runtime, absent when the compiler is reset.
    runtime: Option<Runtime>,
    /// publication/reset options used by the current compilation.
    options: CompilerOptions,
    /// most recent successful compile statistics.
    statistics: Option<CompileStatistics>,
    /// host moving-piston identities tracked between publications.
    exported_motions: HashMap<ExportKey, ExportedMotion>,
    /// moving piston block parts queued for host-world publication.
    publication_parts: Vec<MovingPart>,
    /// positions already included in the current publication.
    publication_positions: HashSet<BlockPos>,
    /// host motion identities retired during reconciliation.
    retired_motions: HashSet<u64>,
    /// moving-piston positions mapped to host motion indices.
    host_motion_indices: HashMap<BlockPos, usize>,
    /// indicates a publication reconciliation pass is in progress.
    publication_phase: bool,
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
        let runtime = sampled::start_runtime(program, ticks, world.piston_state().clone()).map_err(CompileError::Unsupported)?;
        let capacity = runtime.motion_part_capacity();
        self.publication_parts.clear();
        self.publication_parts.reserve(capacity);
        self.publication_positions.clear();
        self.publication_positions.reserve(capacity);
        self.retired_motions.clear();
        self.retired_motions.reserve(capacity);
        self.host_motion_indices.clear();
        self.host_motion_indices.reserve(capacity);
        self.exported_motions.clear();
        self.exported_motions.reserve(capacity);
        self.runtime = Some(runtime);
        self.statistics = Some(CompileStatistics {
            components,
            pending_ticks,
            duration: start.elapsed(),
        });
        self.options = options;
        if std::env::var_os("MCHPRS_REDPILER_PHASE_TIMINGS").is_some() {
            self.runtime.as_ref().unwrap().log_memory_layout();
        }
        Ok(())
    }

    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        if let Err(error) = self.try_reset(world, bounds) {
            tracing::error!(%error, "compiled reset failed; compiler state retained");
        }
    }

    pub fn try_reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) -> Result<(), ExecutionError> {
        if self.runtime.is_some() {
            self.publish(world, false, true)?;
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
        self.publish(world, self.options.io_only, cfg!(feature = "redpiler-full-publication"))
    }

    pub(crate) fn try_flush_complete<W: World>(&mut self, world: &mut W) -> Result<(), ExecutionError> {
        self.publish(world, false, true)
    }

    fn publish<W: World>(&mut self, world: &mut W, io_only: bool, full: bool) -> Result<(), ExecutionError> {
        self.runtime
            .as_mut()
            .expect("compiler inactive")
            .prepare_publication(io_only || !full)
            .map_err(ExecutionError)?;
        let runtime = self.runtime.as_ref().unwrap();
        if full && !io_only {
            runtime
                .write_moving_parts(&mut self.publication_parts, &mut self.publication_positions)
                .map_err(ExecutionError)?;
            // Reserve identities before host setters register any new parts.
            if world
                .piston_state()
                .next_identity
                .checked_add((self.publication_parts.len() as u64).saturating_mul(2))
                .is_none()
            {
                return Err(ExecutionError(ScratchError {
                    position: None,
                    reason: "piston motion identity limit exceeded",
                }));
            }
            self.publication_phase = !self.publication_phase;
            let mut next_identity = world.piston_state().next_identity;
            for part in &self.publication_parts {
                let entry = self.exported_motions.entry(part.key).or_insert_with(|| {
                    next_identity += 1;
                    ExportedMotion {
                        identity: next_identity,
                        seen: self.publication_phase,
                    }
                });
                entry.seen = self.publication_phase;
            }
            world.piston_state_mut().next_identity = next_identity;
            self.retired_motions.clear();
            self.exported_motions.retain(|key, entry| {
                if entry.seen == self.publication_phase {
                    true
                } else {
                    self.retired_motions.insert(entry.identity);
                    world.delete_block_entity(key.3);
                    false
                }
            });
        }
        for (pos, block, comparator) in runtime.publication_changes(full) {
            if io_only && !is_output(block) {
                continue;
            }
            if full && !io_only && self.publication_positions.contains(&pos) {
                continue;
            }
            let previous = world.get_block(pos);
            if !full && previous.registry_id() != block.registry_id() {
                continue;
            }
            if previous != block {
                world.set_block(pos, block);
            }
            if !io_only && let Some(output_strength) = comparator {
                if let Some(BlockEntity::Comparator { output_strength: current }) = world.get_block_entity_mut(pos) {
                    *current = output_strength;
                }
            }
        }
        if full && !io_only {
            for part in &self.publication_parts {
                if world.get_block(part.pos) != part.block {
                    world.set_block(part.pos, part.block);
                }
                if let Some(BlockEntity::MovingPiston(entity)) = world.get_block_entity_mut(part.pos) {
                    *entity = part.entity;
                } else {
                    world.set_block_entity(part.pos, BlockEntity::MovingPiston(part.entity));
                }
            }
            let logical_tick = runtime.logical_tick();
            let state = world.piston_state_mut();
            self.host_motion_indices.clear();
            let mut index = 0;
            state.motions.retain(|motion| {
                if self.retired_motions.contains(&motion.identity) {
                    return false;
                }
                if self.publication_positions.contains(&motion.pos) {
                    if self.host_motion_indices.contains_key(&motion.pos) {
                        return false;
                    }
                    self.host_motion_indices.insert(motion.pos, index);
                }
                index += 1;
                true
            });
            for part in &self.publication_parts {
                let identity = self.exported_motions[&part.key].identity;
                if let Some(&index) = self.host_motion_indices.get(&part.pos) {
                    let motion = &mut state.motions[index];
                    motion.identity = identity;
                    motion.progress = part.progress;
                    motion.previous_progress = (part.progress - 0.5).max(0.0);
                    motion.last_tick = logical_tick;
                } else {
                    state.motions.push_back(PistonMotion {
                        pos: part.pos,
                        identity,
                        progress: part.progress,
                        previous_progress: (part.progress - 0.5).max(0.0),
                        last_tick: logical_tick,
                        carried_entity: None,
                    });
                }
            }
            state.logical_tick = logical_tick;
            state.phase = AdvancePhase::BetweenTicks;
            state.scheduled_advanced = false;
            state.events.retain(|event| !runtime.owns_piston(event.pos));
            state.events.extend(runtime.pending_piston_event_iter());
            state.movement_work.clear();
            state.movement_cursor = 0;
        }
        let runtime = self.runtime.as_mut().unwrap();
        runtime.publish_commands(world, io_only);
        for (component, pos, instrument, note) in runtime.notes.drain(..) {
            redstone::noteblock::play_note(world, pos, instrument, note);
            runtime.note_queued[component] = false;
        }
        runtime.mark_published(io_only || !full);
        Ok(())
    }
}

fn is_output(block: Block) -> bool {
    matches!(block, Block::Lever { .. } | Block::RedstoneLamp { .. })
}
