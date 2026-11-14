//! Validate, prepare and activate fresh programs or saved executors.
use super::{CompileError, CompileStatistics, Compiler};
use crate::redpilator::{
    CompilerOptions, PublicationMode, TaskMonitor,
    sampled::{self, LlvmRuntime, Runtime, ScratchEntry, ScratchError},
};
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_world::{AdvancePhase, TickEntry};
use std::{sync::Arc, time::Instant};

impl Compiler {
    pub fn compile<W: World>(
        &mut self,
        world: &W,
        bounds: (BlockPos, BlockPos),
        options: CompilerOptions,
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
    ) -> Result<(), CompileError> {
        self.compile_cached(world, bounds, options, ticks, monitor, None)
    }

    pub(crate) fn cached_program(&self) -> Option<Arc<sampled::Program>> {
        self.runtime.as_ref().map(Runtime::shared_program)
    }

    #[cfg(test)]
    pub(crate) fn source_sampling_stats(&self) -> Option<(usize, usize, usize)> {
        let program = self.cached_program()?;
        let cache = program.source_order_cache.as_ref()?;
        let (hits, misses) = cache.reuse_counts();
        Some((hits, misses, cache.estimated_bytes()))
    }

    pub(crate) fn refresh_command_output_state<W: World>(&mut self, world: &W) {
        if let Some(runtime) = self.runtime.as_mut() {
            runtime.refresh_command_output_state(world);
            if let Some(native) = self.llvm_runtime.as_mut() {
                native.acknowledge_effects(runtime.queued_command_outputs());
            }
        }
    }

    /// Resume an executor only after its owner verifies the exported world is unchanged.
    ///
    /// Inputs: The current world, requested options, monitor and saved compiler.
    ///
    /// Reject incompatible checkpoints, refresh command output and publication state, then install the
    /// saved executor after a final cancellation check.
    pub(crate) fn resume_cached<W: World>(
        &mut self,
        world: &W,
        mut options: CompilerOptions,
        monitor: Arc<TaskMonitor>,
        mut cached: Self,
    ) -> Result<(), CompileError> {
        if self.is_active() {
            return Err(CompileError::AlreadyActive);
        }
        let started = Instant::now();
        monitor.set_phase("restoring exported executor");
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        options.io_only = options.publication_mode() == PublicationMode::Io;
        if !cached.is_active() || options.update || options.llvm != cached.options.llvm || options.relaxed_mode != cached.options.relaxed_mode {
            return Err(CompileError::Unsupported(ScratchError {
                position: None,
                reason: "incompatible executor checkpoint",
            }));
        }
        let runtime = cached.runtime.as_mut().unwrap();
        runtime.refresh_command_output_state(world);
        runtime.invalidate_publication();
        if let Some(native) = cached.llvm_runtime.as_mut() {
            native.acknowledge_effects(runtime.queued_command_outputs());
        }
        let statistics = cached.statistics.as_mut().unwrap();
        statistics.pending_ticks = runtime.pending_entries().len();
        statistics.duration = started.elapsed();
        statistics.cached = true;
        cached.options = options;
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        *self = cached;
        Ok(())
    }

    pub(crate) fn compile_cached<W: World>(
        &mut self,
        world: &W,
        bounds: (BlockPos, BlockPos),
        options: CompilerOptions,
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
        cached: Option<Arc<sampled::Program>>,
    ) -> Result<(), CompileError> {
        self.compile_cached_with_sampling(world, bounds, options, ticks, monitor, cached, None)
    }

    /// Prepare and activate an executor from a captured circuit.
    ///
    /// Inputs: The world and bounds, options, pending ticks, monitor, and optional program or sampling cache.
    ///
    /// Validate entry state, reuse or compile the program, initialize the selected backend and Rust
    /// projection, then install them only after cancellation checks.
    pub(crate) fn compile_cached_with_sampling<W: World>(
        &mut self,
        world: &W,
        bounds: (BlockPos, BlockPos),
        mut options: CompilerOptions,
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
        cached: Option<Arc<sampled::Program>>,
        source_cache: Option<Arc<sampled::SourceOrderCache>>,
    ) -> Result<(), CompileError> {
        if self.is_active() {
            return Err(CompileError::AlreadyActive);
        }
        options.io_only = options.publication_mode() == PublicationMode::Io;
        let start = Instant::now();
        monitor.set_phase("entry validation");
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        if options.llvm {
            validate_llvm_entry(world.piston_state()).map_err(CompileError::Unsupported)?;
            if let Some(tick) = ticks.first() {
                return Err(CompileError::Unsupported(ScratchError {
                    position: Some(tick.pos),
                    reason: "LLVM requires an idle start with no scheduled component ticks; stop the clock and let the circuit settle before compiling",
                }));
            }
        }
        let entry = ScratchEntry {
            scheduled: ticks.clone(),
            piston: world.piston_state().clone(),
        };
        let cached = cached.filter(|program| program.relaxed_mode == options.relaxed_mode && program.movement_folding == options.movement_folding());
        let cache_hit = cached.is_some();
        let program = if let Some(program) = cached {
            program
        } else {
            compile_program(world, bounds, entry, &options, &monitor, source_cache.as_deref())?
        };
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        let components = program.component_count();
        let pending_ticks = ticks.len();
        let llvm = if options.llvm {
            monitor.set_phase("LLVM code generation and initialization");
            Some(prepare_llvm_runtime(&program, &ticks, world.piston_state().logical_tick, &monitor)?)
        } else {
            None
        };
        monitor.set_phase("initializing runtime");
        let runtime = sampled::start_shared_runtime(program, ticks, world.piston_state().clone()).map_err(CompileError::Unsupported)?;
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        self.prepare_publication_buffers(runtime.motion_part_capacity());
        self.runtime = Some(runtime);
        self.llvm_runtime = llvm;
        self.statistics = Some(CompileStatistics {
            components,
            pending_ticks,
            duration: start.elapsed(),
            cached: cache_hit,
        });
        self.options = options;
        if std::env::var_os("MCHPRS_REDPILATOR_PHASE_TIMINGS").is_some() {
            self.runtime.as_ref().unwrap().log_memory_layout();
        }
        Ok(())
    }
}

fn compile_program(
    world: &impl World,
    bounds: (BlockPos, BlockPos),
    entry: ScratchEntry,
    options: &CompilerOptions,
    monitor: &TaskMonitor,
    source_cache: Option<&sampled::SourceOrderCache>,
) -> Result<Arc<sampled::Program>, CompileError> {
    let mut program = sampled::compile_program_with_source_cache(world, bounds, entry, Some(monitor), source_cache)
        .map_err(|error| compile_failure(error, monitor))?;
    if options.relaxed_mode {
        program.enable_relaxed_wire_shapes().map_err(CompileError::Unsupported)?;
    }
    if options.movement_folding() {
        monitor.set_phase("folding equivalent movement states");
        program.fold_movement_states().map_err(CompileError::Unsupported)?;
    }
    Ok(Arc::new(program))
}

fn compile_failure(error: ScratchError, monitor: &TaskMonitor) -> CompileError {
    if monitor.cancelled() {
        CompileError::Cancelled
    } else {
        CompileError::Unsupported(error)
    }
}

fn validate_llvm_entry(piston: &mchprs_world::PistonState) -> Result<(), ScratchError> {
    let has_pending_movement =
        !piston.events.is_empty() || !piston.motions.is_empty() || !piston.movement_work.is_empty() || piston.movement_cursor != 0;
    if !has_pending_movement && piston.phase == AdvancePhase::BetweenTicks {
        return Ok(());
    }
    let position = piston
        .events
        .front()
        .map(|event| event.pos)
        .or_else(|| piston.motions.front().map(|motion| motion.pos))
        .or_else(|| piston.movement_work.get(piston.movement_cursor).map(|&(pos, _)| pos));
    let reason = match piston.phase {
        AdvancePhase::ScheduledTicks => "LLVM cannot compile during scheduled ticks; finish the current tick first",
        AdvancePhase::PistonEvents => "LLVM cannot compile during piston events; finish the current tick first",
        AdvancePhase::MovingEntities => "LLVM cannot compile during piston movement; finish the current tick first",
        AdvancePhase::BetweenTicks => "LLVM cannot import pending piston work; let it settle before compiling",
    };
    Err(ScratchError { position, reason })
}

fn prepare_llvm_runtime(
    program: &sampled::Program,
    ticks: &[TickEntry],
    logical_tick: u64,
    monitor: &TaskMonitor,
) -> Result<LlvmRuntime, CompileError> {
    let (first, total) = sampled::unsupported_components(program);
    if total > 1 {
        return Err(CompileError::UnsupportedComponents { first, total });
    }
    let mut runtime = LlvmRuntime::new_with_monitor(program, ticks, Some(monitor)).map_err(|error| compile_failure(error, monitor))?;
    runtime.initialize_tick(logical_tick);
    Ok(runtime)
}
