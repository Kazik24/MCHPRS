//! Compile an isolated snapshot while the plot continues handling players and commands.
use super::*;
use crate::redpilator::{CachedProgram, CompileError, TaskMonitor};
use std::sync::Arc;
use std::thread::JoinHandle;

const PROGRESS_INTERVAL: Duration = Duration::from_secs(3);

#[cfg(test)]
mod tests;

pub(super) struct CompilationJob {
    /// Time the request started, including snapshot preparation.
    started: Instant,
    last_progress: Instant,
    /// Cooperative cancellation shared with the compiler worker.
    monitor: Arc<TaskMonitor>,
    budget: Option<Duration>,
    budget_reported: bool,
    /// Live entry state that must still match before activation.
    entry: EntryVersion,
    options: CompilerOptions,
    /// Worker owning all temporary compilation data and no live plot references.
    worker: JoinHandle<CompilationResult>,
}

impl CompilationJob {
    fn spawn(
        world: PlotWorld,
        entry: EntryVersion,
        options: CompilerOptions,
        budget: Option<Duration>,
        started: Instant,
        cached: Option<CompilationCache>,
    ) -> std::io::Result<Self> {
        let monitor = Arc::new(TaskMonitor::with_deadline(budget.map(|budget| started + budget)));
        let worker_monitor = monitor.clone();
        let worker_options = options.clone();
        let worker_entry = entry.clone();
        let span = tracing::Span::current();
        let worker = thread::Builder::new().name(format!("compile-{},{}", world.x, world.z)).spawn(move || {
            let _entered = span.enter();
            CompilationResult::compile(world, worker_entry, worker_options, worker_monitor, cached)
        })?;
        Ok(Self {
            started,
            last_progress: Instant::now(),
            monitor,
            budget,
            budget_reported: false,
            entry,
            options,
            worker,
        })
    }

    fn take_timeout_message(&mut self) -> Option<String> {
        let budget = self.budget?;
        if self.budget_reported || !self.monitor.budget_expired() {
            return None;
        }
        self.budget_reported = true;
        Some(format!(
            "Compilation exceeded your {} second budget. Try a smaller circuit.",
            budget.as_secs()
        ))
    }

    fn take_progress_message(&mut self) -> Option<String> {
        if self.worker.is_finished() || self.monitor.cancelled() || self.last_progress.elapsed() < PROGRESS_INTERVAL {
            return None;
        }
        self.last_progress = Instant::now();
        let elapsed_secs = self.started.elapsed().as_secs();
        let status = self.monitor.status();
        info!(elapsed_secs, %status, "Redpilator compilation progress");
        Some(messages::redpilator_progress(elapsed_secs, &status))
    }
}

pub(super) struct CompilationCache {
    entry: EntryVersion,
    options: CompilerOptions,
    program: Arc<CachedProgram>,
    /// Present after reset: the executor already represents the exported world state.
    suspended: Option<Compiler>,
}

impl CompilationCache {
    pub(super) fn exported(world: &PlotWorld, compiler: Compiler) -> Self {
        Self {
            entry: EntryVersion::capture(world),
            options: compiler.current_flags().unwrap().clone(),
            program: compiler.cached_program().unwrap(),
            suspended: Some(compiler),
        }
    }
}

struct CompilationResult {
    /// Snapshot used to prepare the executor.
    world: PlotWorld,
    /// Prepared executor transferred to the plot only on success.
    compiler: Compiler,
    /// Structured rejection retained for diagnostics and block highlighting.
    result: Result<(), CompileError>,
}

impl CompilationResult {
    fn compile(world: PlotWorld, entry: EntryVersion, options: CompilerOptions, monitor: Arc<TaskMonitor>, cached: Option<CompilationCache>) -> Self {
        let mut compiler = Compiler::default();
        let cached = cached.filter(|cache| cache.options == options);
        let source_cache = cached.as_ref().and_then(|cache| cache.program.source_order_cache.clone());
        let cached = cached.filter(|cache| cache.entry == entry);
        let result = match cached {
            Some(CompilationCache { suspended: Some(saved), .. }) => compiler.resume_cached(&world, options, monitor, saved),
            cache => {
                let ticks = world.to_be_ticked.iter_entries().collect();
                compiler.compile_cached_with_sampling(
                    &world,
                    world.get_corners(),
                    options,
                    ticks,
                    monitor,
                    cache.map(|cache| cache.program),
                    source_cache,
                )
            }
        };
        Self { world, compiler, result }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct EntryVersion {
    bounds: (BlockPos, BlockPos),
    /// All plot chunks also cover the compiler's read-only probe context.
    chunks: Vec<(u64, u64)>,
    /// Ordered pending work detects scheduling changes without block edits.
    ticks: Vec<TickEntry>,
    /// Complete piston state detects partial-tick changes without requiring new equality traits.
    pistons: Vec<u8>,
    cursed: bool,
    /// Queue draining is harmless; changed rate-limit accounting requires a new capture.
    command_output: Option<(Instant, usize, usize, bool)>,
}

impl EntryVersion {
    fn capture(world: &PlotWorld) -> Self {
        Self {
            bounds: world.get_corners(),
            chunks: world.chunks.iter().map(Chunk::circuit_snapshot_version).collect(),
            ticks: world.to_be_ticked.iter_entries().collect(),
            pistons: bincode::serialize(&world.piston_state).expect("piston state serializes to memory"),
            cursed: world.is_cursed,
            command_output: Self::command_output(world),
        }
    }

    fn command_output(world: &PlotWorld) -> Option<(Instant, usize, usize, bool)> {
        world
            .command_block_output_state()
            .map(|state| (state.window, state.count, state.bytes, state.limited))
    }

    fn matches(&self, world: &PlotWorld) -> bool {
        self.bounds == world.get_corners()
            && self.chunks.iter().copied().eq(world.chunks.iter().map(Chunk::circuit_snapshot_version))
            && self.ticks.iter().cloned().eq(world.to_be_ticked.iter_entries())
            && self.pistons == bincode::serialize(&world.piston_state).expect("piston state serializes to memory")
            && self.cursed == world.is_cursed
            && self.command_output == Self::command_output(world)
    }
}

fn rejected_position(error: &CompileError) -> Option<BlockPos> {
    match error {
        CompileError::Unsupported(error) => error.position,
        CompileError::UnsupportedComponents { first, .. } => first.iter().find_map(|error| error.position),
        _ => None,
    }
}

fn compilation_error(world: &PlotWorld, error: &CompileError, monitor: &TaskMonitor) -> String {
    let detail = if let CompileError::Unsupported(issue) = error
        && let Some(position) = issue.position
    {
        let block = world.get_block(position).get_name();
        if issue.reason == block {
            messages::redpilator_unsupported_block(block, position)
        } else {
            messages::redpilator_block_failure(block, position, issue.reason)
        }
    } else {
        error.to_string()
    };
    let mut text = messages::redpilator_failed_phase(&monitor.status(), detail);
    if let Some(hint) = error.recovery_hint() {
        text.push('\n');
        text.push_str(hint);
    }
    text
}

impl Plot {
    pub(super) fn clear_redpilator_cache(&mut self) {
        self.cancel_redpilator_compilation();
        self.redpilator_cache = None;
    }

    pub(super) fn compiling_redpilator(&self) -> bool {
        self.compilation.as_ref().is_some_and(|job| !job.monitor.cancelled())
    }

    pub(super) fn cancel_redpilator_compilation(&mut self) {
        if let Some(job) = &self.compilation {
            job.monitor.cancel();
        }
    }

    pub(super) fn start_automatic_redpilator(&mut self, options: CompilerOptions) {
        let budget = self
            .players
            .iter()
            .filter(|player| {
                player.has_permission("commands.redpilator.compile")
                    && player.has_permission("commands.toggleautorp")
                    && player.can_edit_plot(self.owner, (self.world.x, self.world.z))
            })
            .filter_map(|player| player.redpilator_compile_budget().ok())
            .max_by_key(|budget| budget.unwrap_or(Duration::MAX));
        if let Some(budget) = budget {
            self.start_redpilator_with_budget(options, budget);
        }
    }

    pub(super) fn start_redpilator_with_budget(&mut self, mut options: CompilerOptions, budget: Option<Duration>) {
        options.io_only = options.publication_mode() == crate::redpilator::PublicationMode::Io;
        if self.world.waveform.enabled() {
            for player in &self.players {
                player.send_error_message("Disable waveform capture before compiling Redpilator");
            }
            return;
        }
        // Keep cancelled workers until they finish so repeated requests cannot multiply large jobs.
        if self.compilation.is_some() {
            for player in &self.players {
                player.send_error_message(messages::REDPILATOR_COMPILATION_BUSY);
            }
            return;
        }
        let started = Instant::now();
        self.close_all_containers();
        let entry = EntryVersion::capture(&self.world);
        let world = self.world.activation_snapshot();
        let cached = self.redpilator_cache.take().filter(|cache| cache.options == options);
        let snapshot_ms = started.elapsed().as_millis();
        info!(plot_x = self.world.x, plot_z = self.world.z, snapshot_ms, ?options, "Redpilator started");
        self.scoreboard.set_redpilator_state(&self.players, RedpilatorState::Compiling);
        self.scoreboard.set_redpilator_options(&self.players, &options);
        let milestone = format!(
            "Redpilator: snapshot ready ({} ms); {}",
            snapshot_ms,
            if cached.is_some() {
                "reusing compiled circuit"
            } else {
                "compiling circuit"
            }
        );
        self.send_redpilator_diagnostic(&milestone);

        match CompilationJob::spawn(world, entry, options, budget, started, cached) {
            Ok(job) => self.compilation = Some(job),
            Err(error) => self.reject_redpilator(started, &format!("cannot start compiler worker: {error}")),
        }
        self.reset_timings();
    }

    pub(super) fn poll_redpilator_compilation(&mut self) {
        if let Some(job) = &mut self.compilation
            && let Some(reason) = job.take_timeout_message()
        {
            let started = job.started;
            self.reject_redpilator(started, &reason);
        }
        if let Some(job) = &mut self.compilation
            && let Some(message) = job.take_progress_message()
        {
            self.send_redpilator_diagnostic(&message);
        }
        if !self.compilation.as_ref().is_some_and(|job| job.worker.is_finished()) {
            return;
        }
        let job = self.compilation.take().unwrap();
        let completed = job.worker.join();

        if job.monitor.cancelled() {
            self.reset_timings();
            return;
        }
        let output = match completed {
            Ok(output) => output,
            Err(_) => {
                self.reject_redpilator(
                    job.started,
                    &messages::redpilator_failed_phase(&job.monitor.status(), messages::REDPILATOR_WORKER_FAILED),
                );
                return;
            }
        };

        if let Err(error) = output.result {
            if let Some(position) = rejected_position(&error) {
                for player in 0..self.players.len() {
                    self.show_block_highlight(player, position, false);
                }
            }
            self.reject_redpilator(job.started, &compilation_error(&output.world, &error, &job.monitor));
        } else if !job.entry.matches(&self.world) || self.git_checkout_locked() {
            self.reject_redpilator(job.started, messages::REDPILATOR_COMPILATION_CHANGED);
        } else {
            self.activate_redpilator(output, job.entry, job.options, job.started);
        }
    }

    fn activate_redpilator(&mut self, mut output: CompilationResult, entry: EntryVersion, options: CompilerOptions, started: Instant) {
        output.compiler.refresh_command_output_state(&self.world);
        self.redpilator_cache = output.compiler.cached_program().map(|program| CompilationCache {
            entry,
            options,
            program,
            suspended: None,
        });
        self.redpilator = output.compiler;
        self.redpilator_handoff_failed = false;
        let state = if self.redpilator.current_flags().is_some_and(|options| options.llvm) {
            RedpilatorState::RunningLlvm
        } else {
            RedpilatorState::Running
        };
        if let Some(stats) = self.redpilator.stats() {
            for line in stats.chat_summary_lines() {
                self.send_redpilator_diagnostic(&line);
            }
        }
        self.world.publish_activation_changes(&output.world);
        self.world.clear_scheduled_ticks();
        self.close_all_containers_after_redpilator();
        if self.world.history.enabled() {
            let bytes = self.world.history.disable();
            self.broadcast_plot_chat_message(&messages::history_disabled_for_compilation(history::format_memory(bytes)));
        }
        self.scoreboard.set_redpilator_state(&self.players, state);
        info!(
            plot_x = self.world.x,
            plot_z = self.world.z,
            elapsed_ms = started.elapsed().as_millis(),
            "Redpilator completed"
        );
        self.reset_timings();
    }

    fn send_redpilator_diagnostic(&self, text: &str) {
        let message = crate::player::diagnostic_component(text).to_string();
        for player in &self.players {
            player.send_raw_system_message(message.clone());
        }
    }

    fn reject_redpilator(&mut self, started: Instant, reason: &str) {
        warn!(plot_x = self.world.x, plot_z = self.world.z,
            elapsed_ms = started.elapsed().as_millis(), %reason, "Redpilator rejected");
        for player in &self.players {
            player.send_error_message(&messages::redpilator_rejected(started.elapsed(), reason));
        }
        self.scoreboard.set_redpilator_state(&self.players, RedpilatorState::Stopped);
        self.scoreboard.set_redpilator_options(&self.players, &Default::default());
        self.reset_timings();
    }
}
