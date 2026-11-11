mod publication;
use publication::ExportedMotion;

use super::{
    CompilerOptions, PublicationMode, TaskMonitor,
    sampled::{self, ExportKey, LlvmRuntime, MovingPart, Runtime, ScratchEntry, ScratchError},
};
use crate::redstone;
use crate::world::{World, for_each_block_mut_optimized};
use mchprs_blocks::BlockPos;
use mchprs_world::{AdvancePhase, TickEntry};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum CompileError {
    AlreadyActive,
    Unsupported(ScratchError),
    UnsupportedComponents { first: Vec<ScratchError>, total: usize },
    Cancelled,
}

impl std::fmt::Display for CompileError {
    /// Format a compilation rejection for the caller.
    ///
    /// Inputs: The error variant and destination formatter.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActive => f.write_str("reset the active compiler before recompiling"),
            Self::Unsupported(error) => {
                if let Some(position) = error.position {
                    write!(f, "Cannot compile at {position}: {}", error.reason)
                } else {
                    f.write_str(error.reason)
                }
            }
            Self::UnsupportedComponents { first, total } => {
                for (index, error) in first.iter().enumerate() {
                    if index > 0 {
                        f.write_str("; ")?;
                    }
                    if let Some(pos) = error.position {
                        write!(f, "{} at ({}, {}, {})", error.reason, pos.x, pos.y, pos.z)?;
                    } else {
                        f.write_str(error.reason)?;
                    }
                }
                if *total > first.len() {
                    write!(f, "; and {} other unsupported LLVM components", total - first.len())?;
                }
                Ok(())
            }
            Self::Cancelled => f.write_str("compilation cancelled"),
        }
    }
}

impl std::error::Error for CompileError {}

impl CompileError {
    /// Choose a useful next action after compilation fails.
    ///
    /// Inputs: The structured compilation error and its reason text.
    ///
    /// Match known backend, entry-state and capture-boundary failures to the corresponding recovery
    /// message.
    pub(crate) fn recovery_hint(&self) -> Option<&'static str> {
        use crate::messages;
        let reason = match self {
            Self::UnsupportedComponents { .. } => return Some(messages::REDPILATOR_RETRY_RUST),
            Self::Unsupported(error) => error.reason,
            _ => return None,
        };
        if reason.starts_with("LLVM does not support") {
            Some(messages::REDPILATOR_RETRY_RUST)
        } else if reason.starts_with("LLVM cannot compile during")
            || reason == "LLVM cannot import pending piston work; let it settle before compiling"
            || matches!(
                reason,
                "scratch entry is mid-tick"
                    | "scratch entry has queued piston work"
                    | "scratch entry has piston motion"
                    | "scratch entry has pending movement work"
            )
        {
            Some(messages::REDPILATOR_RETRY_SETTLED)
        } else if matches!(
            reason,
            "LLVM DLL ABI/version mismatch"
                | "LLVM DLL is missing a required export"
                | "LLVM module is missing a required export"
                | "could not load LLVM DLL (set MCHPRS_LLVM_DLL)"
                | "could not load LLVM module"
        ) {
            Some(messages::REDPILATOR_REBUILD_LLVM)
        } else if reason.contains("outside compiled region") || reason.contains("outside captured boundary") {
            Some(messages::REDPILATOR_CAPTURE_WHOLE_CIRCUIT)
        } else if matches!(
            reason,
            "invalid native Program descriptor" | "LLVM rejected the sampled Program" | "LLVM state size rejected"
        ) {
            Some(messages::REDPILATOR_INTERNAL_FAILURE_HELP)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionError(
    /// underlying runtime failure, including its optional world position and reason.
    ScratchError,
);

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(position) = self.0.position {
            write!(f, "Compiled execution failed at {position}: {}", self.0.reason)
        } else {
            write!(f, "Compiled execution failed: {}", self.0.reason)
        }
    }
}

impl std::error::Error for ExecutionError {}

#[derive(Debug, Clone)]
pub struct CompileStatistics {
    /// number of compiled redstone components.
    pub components: usize,
    /// number of host scheduled ticks copied into the compiled runtime.
    pub pending_ticks: usize,
    /// elapsed compile time.
    pub duration: Duration,
    /// Whether a prepared program or an exported executor checkpoint was reused.
    pub cached: bool,
}

impl CompileStatistics {
    pub fn chat_summary_lines(&self) -> Vec<String> {
        vec![format!(
            "Redpilator {} {} components; {} pending ticks; {:?}",
            if self.cached { "reused" } else { "compiled" },
            self.components,
            self.pending_ticks,
            self.duration
        )]
    }
}

#[derive(Default)]
pub struct Compiler {
    /// Rust executor, or a read-only publication projection of the LLVM executor.
    pub(super) runtime: Option<Runtime>,
    /// active experimental LLVM executor; ticks use it when present.
    pub(super) llvm_runtime: Option<LlvmRuntime>,
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
    /// Experimental benchmark helper: the unticked seed and pristine world must describe the same fixture.
    #[cfg(feature = "plot-bench")]
    pub fn benchmark_restart<W: World>(&self, pristine: &W, bounds: (BlockPos, BlockPos), ticks: Vec<TickEntry>) -> Result<Self, CompileError> {
        let mut fresh = Self::default();
        fresh.compile_cached(
            pristine,
            bounds,
            self.options.clone(),
            ticks,
            Arc::new(TaskMonitor::default()),
            self.cached_program(),
        )?;
        Ok(fresh)
    }

    #[cfg(feature = "plot-bench")]
    pub fn benchmark_tick_batch(&mut self, count: u32, mode: u32) -> Result<(), ExecutionError> {
        self.llvm_runtime
            .as_mut()
            .expect("batch experiment requires LLVM")
            .benchmark_tick_batch(count, mode)
            .map_err(ExecutionError)
    }

    #[cfg(feature = "plot-bench")]
    pub fn benchmark_snapshot(&self) -> Result<Vec<u32>, ExecutionError> {
        self.llvm_runtime
            .as_ref()
            .expect("batch experiment requires LLVM")
            .benchmark_snapshot()
            .map_err(ExecutionError)
    }

    pub fn stats(&self) -> Option<&CompileStatistics> {
        self.statistics.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.runtime.is_some() || self.llvm_runtime.is_some()
    }

    pub(crate) fn uses_llvm(&self) -> bool {
        self.llvm_runtime.is_some()
    }

    /// Run one normal native iteration, retaining the completed prefix if a tick fails.
    pub(crate) fn tick_batch_16(&mut self) -> (u32, Result<(), ExecutionError>) {
        let mut completed = 0;
        let result = if let Some(native) = self.llvm_runtime.as_mut() {
            native.tick_batch(16, &mut completed).map_err(ExecutionError)
        } else {
            (|| {
                for _ in 0..16 {
                    self.tick()?;
                    completed += 1;
                }
                Ok(())
            })()
        };
        (completed, result)
    }

    pub fn current_flags(&self) -> Option<&CompilerOptions> {
        self.is_active().then_some(&self.options)
    }

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
        }
        let entry = ScratchEntry {
            scheduled: ticks.clone(),
            piston: world.piston_state().clone(),
        };
        let cached = cached.filter(|program| program.relaxed_mode == options.relaxed_mode);
        let cache_hit = cached.is_some();
        let program = if let Some(program) = cached {
            program
        } else {
            let mut program =
                sampled::compile_program_with_source_cache(world, bounds, entry, Some(&monitor), source_cache.as_deref()).map_err(|error| {
                    if monitor.cancelled() {
                        CompileError::Cancelled
                    } else {
                        CompileError::Unsupported(error)
                    }
                })?;
            if options.relaxed_mode {
                program.enable_relaxed_wire_shapes().map_err(CompileError::Unsupported)?;
            }
            Arc::new(program)
        };
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        let components = program.component_count();
        let pending_ticks = ticks.len();
        let llvm = if options.llvm {
            monitor.set_phase("LLVM code generation and initialization");
            Some(prepare_llvm_runtime(&program, &ticks, world.piston_state().logical_tick)?)
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

    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        if let Err(error) = self.try_reset(world, bounds) {
            tracing::error!(%error, "compiled reset failed; compiler state retained");
        }
    }

    pub fn try_reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) -> Result<(), ExecutionError> {
        self.export_for_reset(world)?;
        self.runtime = None;
        self.llvm_runtime = None;
        self.exported_motions.clear();
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

    /// Export reset state while retaining all topology, execution and publication bookkeeping.
    pub(crate) fn try_suspend<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) -> Result<Option<Self>, ExecutionError> {
        if !self.is_active() || self.options.update {
            self.try_reset(world, bounds)?;
            return Ok(None);
        }
        self.export_for_reset(world)?;
        Ok(Some(std::mem::take(self)))
    }

    fn export_for_reset<W: World>(&mut self, world: &mut W) -> Result<(), ExecutionError> {
        if self.runtime.is_some() {
            self.publish(world, false, true)?;
            let runtime = self.runtime.as_ref().unwrap();
            #[cfg(feature = "redpilator-trace")]
            runtime.log_routing_metrics();
            let pending = runtime.pending_entries();
            for entry in pending {
                world.restore_scheduled_tick(entry);
            }
        }
        Ok(())
    }

    pub fn tick(&mut self) -> Result<(), ExecutionError> {
        if let Some(runtime) = self.llvm_runtime.as_mut() {
            return runtime.tick().map_err(ExecutionError);
        }
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
        if let Some(runtime) = self.llvm_runtime.as_mut() {
            return runtime.use_control(pos).map_err(ExecutionError);
        }
        self.runtime.as_mut().expect("compiler inactive").use_control(pos).map_err(ExecutionError)
    }

    pub fn try_set_pressure_plate(&mut self, pos: BlockPos, powered: bool) -> Result<(), ExecutionError> {
        if let Some(runtime) = self.llvm_runtime.as_mut() {
            return runtime.set_pressure_plate(pos, powered).map_err(ExecutionError);
        }
        self.runtime
            .as_mut()
            .expect("compiler inactive")
            .set_pressure_plate(pos, powered)
            .map_err(ExecutionError)
    }

    pub(crate) fn accepts_control(&self, pos: BlockPos) -> bool {
        self.runtime.as_ref().is_some_and(|runtime| runtime.accepts_control(pos))
            || self.llvm_runtime.as_ref().is_some_and(|runtime| runtime.accepts_control(pos))
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

fn prepare_llvm_runtime(program: &sampled::Program, ticks: &[TickEntry], logical_tick: u64) -> Result<LlvmRuntime, CompileError> {
    let (first, total) = sampled::unsupported_components(program);
    if total > 1 {
        return Err(CompileError::UnsupportedComponents { first, total });
    }
    let mut runtime = LlvmRuntime::new(program, ticks).map_err(CompileError::Unsupported)?;
    runtime.initialize_tick(logical_tick);
    Ok(runtime)
}
