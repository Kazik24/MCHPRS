//! Compile an isolated snapshot while the plot continues handling players and commands.
use super::*;
use crate::redpiler::{CachedProgram, CompileError, TaskMonitor};
use std::sync::Arc;
use std::thread::JoinHandle;

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
    cacheable: bool,
    /// Worker owning all temporary compilation data and no live plot references.
    worker: JoinHandle<CompilationResult>,
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
            messages::redpiler_unsupported_block(block, position)
        } else {
            messages::redpiler_block_failure(block, position, issue.reason)
        }
    } else {
        error.to_string()
    };
    let mut text = messages::redpiler_failed_phase(&monitor.status(), detail);
    if let Some(hint) = error.recovery_hint() {
        text.push('\n');
        text.push_str(hint);
    }
    text
}

impl Plot {
    pub(super) fn clear_redpiler_cache(&mut self) {
        self.cancel_redpiler_compilation();
        self.redpiler_cache = None;
    }

    pub(super) fn compiling_redpiler(&self) -> bool {
        self.compilation.as_ref().is_some_and(|job| !job.monitor.cancelled())
    }

    pub(super) fn cancel_redpiler_compilation(&mut self) {
        if let Some(job) = &self.compilation {
            job.monitor.cancel();
        }
    }

    pub(super) fn start_automatic_redpiler(&mut self, options: CompilerOptions) {
        let budget = self
            .players
            .iter()
            .filter(|player| {
                player.has_permission("commands.redpiler.compile")
                    && player.has_permission("commands.toggleautorp")
                    && player.can_edit_plot(self.owner, (self.world.x, self.world.z))
            })
            .filter_map(|player| player.redpiler_compile_budget().ok())
            .max_by_key(|budget| budget.unwrap_or(Duration::MAX));
        if let Some(budget) = budget {
            self.start_redpiler_with_budget(options, budget);
        }
    }

    pub(super) fn start_redpiler_with_budget(&mut self, mut options: CompilerOptions, budget: Option<Duration>) {
        options.io_only = options.publication_mode() == crate::redpiler::PublicationMode::Io;
        if self.world.waveform.enabled() {
            for player in &self.players {
                player.send_error_message("Disable waveform capture before compiling Redpiler");
            }
            return;
        }
        // Keep cancelled workers until they finish so repeated requests cannot multiply large jobs.
        if self.compilation.is_some() {
            for player in &self.players {
                player.send_error_message(messages::REDPILER_COMPILATION_BUSY);
            }
            return;
        }
        let started = Instant::now();
        if let Err(error) = crate::redpiler::memory::check(0) {
            self.reject_redpiler(started, error.reason);
            return;
        }
        self.close_all_containers();
        let entry = EntryVersion::capture(&self.world);
        let world = self.world.activation_snapshot();
        let cached = self
            .redpiler_cache
            .take()
            .filter(|cache| cache.options == options && cache.entry.matches(&self.world));
        let job_options = options.clone();
        let snapshot_ms = started.elapsed().as_millis();
        let monitor = Arc::new(TaskMonitor::with_deadline(budget.map(|budget| started + budget)));
        let worker_monitor = monitor.clone();
        let span = tracing::Span::current();
        info!(plot_x = self.world.x, plot_z = self.world.z, snapshot_ms, ?options, "Redpiler started");
        self.scoreboard.set_redpiler_state(&self.players, RedpilerState::Compiling);
        self.scoreboard.set_redpiler_options(&self.players, &options);
        let milestone = format!(
            "Redpiler: snapshot ready ({} ms); {}",
            snapshot_ms,
            if cached.is_some() {
                "reusing compiled circuit"
            } else {
                "compiling circuit"
            }
        );
        for player in &self.players {
            player.send_raw_system_message(crate::player::diagnostic_component(&milestone).to_string());
        }

        let worker = thread::Builder::new()
            .name(format!("compile-{},{}", self.world.x, self.world.z))
            .spawn(move || {
                let _entered = span.enter();
                let ticks = world.to_be_ticked.iter_entries().collect();
                let mut compiler = Compiler::default();
                let result = match cached {
                    Some(CompilationCache { suspended: Some(saved), .. }) => compiler.resume_cached(&world, options, worker_monitor, saved),
                    cache => compiler.compile_cached(
                        &world,
                        world.get_corners(),
                        options,
                        ticks,
                        worker_monitor,
                        cache.map(|cache| cache.program),
                    ),
                };
                CompilationResult { world, compiler, result }
            });
        match worker {
            Ok(worker) => {
                self.compilation = Some(CompilationJob {
                    started,
                    last_progress: Instant::now(),
                    monitor,
                    budget,
                    budget_reported: false,
                    entry,
                    options: job_options,
                    cacheable: true,
                    worker,
                })
            }
            Err(error) => self.reject_redpiler(started, &format!("cannot start compiler worker: {error}")),
        }
        self.reset_timings();
    }

    pub(super) fn poll_redpiler_compilation(&mut self) {
        if let Some(job) = &mut self.compilation
            && job.monitor.budget_expired()
            && !job.budget_reported
        {
            job.budget_reported = true;
            let started = job.started;
            let reason = format!(
                "Compilation exceeded your {} second budget. Try a smaller circuit.",
                job.budget.unwrap().as_secs()
            );
            self.reject_redpiler(started, &reason);
        }
        if let Some(job) = &mut self.compilation
            && !job.worker.is_finished()
            && !job.monitor.cancelled()
            && job.last_progress.elapsed() >= Duration::from_secs(3)
        {
            job.last_progress = Instant::now();
            let status = job.monitor.status();
            let text = messages::redpiler_progress(job.started.elapsed().as_secs(), &status);
            info!(elapsed_secs = job.started.elapsed().as_secs(), %status, "Redpiler compilation progress");
            for player in &self.players {
                player.send_raw_system_message(crate::player::diagnostic_component(&text).to_string());
            }
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
                self.reject_redpiler(
                    job.started,
                    &messages::redpiler_failed_phase(&job.monitor.status(), messages::REDPILER_WORKER_FAILED),
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
            self.reject_redpiler(job.started, &compilation_error(&output.world, &error, &job.monitor));
        } else if !job.entry.matches(&self.world) || self.git_checkout_locked() {
            self.reject_redpiler(job.started, messages::REDPILER_COMPILATION_CHANGED);
        } else {
            if job.cacheable {
                self.redpiler_cache = output.compiler.cached_program().map(|program| CompilationCache {
                    entry: job.entry,
                    options: job.options,
                    program,
                    suspended: None,
                });
            }
            self.redpiler = output.compiler;
            self.redpiler_handoff_failed = false;
            let state = if self.redpiler.current_flags().is_some_and(|options| options.llvm) {
                RedpilerState::RunningLlvm
            } else {
                RedpilerState::Running
            };
            if let Some(stats) = self.redpiler.stats() {
                for line in stats.chat_summary_lines() {
                    for player in &self.players {
                        player.send_raw_system_message(crate::player::diagnostic_component(&line).to_string());
                    }
                }
            }
            self.world.publish_activation_changes(&output.world);
            self.world.clear_scheduled_ticks();
            self.close_all_containers_after_redpiler();
            if self.world.history.enabled() {
                let bytes = self.world.history.disable();
                self.broadcast_plot_chat_message(&messages::history_disabled_for_compilation(history::format_memory(bytes)));
            }
            self.scoreboard.set_redpiler_state(&self.players, state);
            info!(
                plot_x = self.world.x,
                plot_z = self.world.z,
                elapsed_ms = job.started.elapsed().as_millis(),
                "Redpiler completed"
            );
            self.reset_timings();
        }
    }

    fn reject_redpiler(&mut self, started: Instant, reason: &str) {
        warn!(plot_x = self.world.x, plot_z = self.world.z,
            elapsed_ms = started.elapsed().as_millis(), %reason, "Redpiler rejected");
        for player in &self.players {
            player.send_error_message(&messages::redpiler_rejected(started.elapsed(), reason));
        }
        self.scoreboard.set_redpiler_state(&self.players, RedpilerState::Stopped);
        self.scoreboard.set_redpiler_options(&self.players, &Default::default());
        self.reset_timings();
    }
}
