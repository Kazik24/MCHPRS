//! Compile an isolated snapshot while the plot continues handling players and commands.
use super::*;
use crate::redpiler::{CompileError, TaskMonitor};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

static ANALYSIS_WORKERS: AtomicUsize = AtomicUsize::new(0);
const MAX_ANALYSIS_WORKERS: usize = 2;

struct AnalysisWorkerPermit;
impl Drop for AnalysisWorkerPermit {
    fn drop(&mut self) {
        ANALYSIS_WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct AnalysisJob {
    started: Instant,
    player: crate::player::EntityId,
    monitor: Arc<TaskMonitor>,
    worker: JoinHandle<Result<AnalysisOutput, String>>,
}

struct AnalysisOutput {
    lines: Vec<String>,
    error: Option<String>,
}

#[cfg(test)]
mod tests;

pub(super) struct CompilationJob {
    /// Time the request started, including snapshot preparation.
    started: Instant,
    /// Cooperative cancellation shared with the compiler worker.
    monitor: Arc<TaskMonitor>,
    /// Live entry state that must still match before activation.
    entry: EntryVersion,
    /// Selected public tier label for the completion summary.
    mode: String,
    /// Worker owning all temporary compilation data and no live plot references.
    worker: JoinHandle<CompilationResult>,
}

struct CompilationResult {
    /// Snapshot containing staged container-close notifications.
    world: PlotWorld,
    /// Prepared executor transferred to the plot only on success.
    compiler: Compiler,
    /// Structured rejection retained for diagnostics and block highlighting.
    result: Result<(), CompileError>,
}

struct EntryVersion {
    /// Chunk identities and revisions detect edits, entity changes and world replacement.
    chunks: Vec<(u64, u64)>,
    /// Ordered pending work detects scheduling changes without block edits.
    ticks: Vec<TickEntry>,
    /// Complete piston state detects partial-tick changes without requiring new equality traits.
    pistons: Vec<u8>,
}

impl EntryVersion {
    fn capture(world: &PlotWorld) -> Self {
        Self {
            chunks: world.chunks.iter().map(Chunk::circuit_snapshot_version).collect(),
            ticks: world.to_be_ticked.iter_entries().collect(),
            pistons: bincode::serialize(&world.piston_state).expect("piston state serializes to memory"),
        }
    }

    fn matches(&self, world: &PlotWorld) -> bool {
        self.chunks.iter().copied().eq(world.chunks.iter().map(Chunk::circuit_snapshot_version))
            && self.ticks.iter().cloned().eq(world.to_be_ticked.iter_entries())
            && self.pistons == bincode::serialize(&world.piston_state).expect("piston state serializes to memory")
    }
}

fn rejected_piston(error: &CompileError, world: &PlotWorld) -> Option<BlockPos> {
    use crate::redpiler::analysis::{AdmissionIssue, geometry::GeometryError};
    use crate::redpiler::backend::graph::GraphBackendError;
    match error {
        CompileError::Unsupported(report) => report.pistons.first().copied(),
        CompileError::RichGraph(error) => match &error.0 {
            GraphBackendError::Geometry(GeometryError::Entry(AdmissionIssue::PendingPistonEvents { .. })) => {
                world.piston_state.events.front().map(|event| event.pos)
            }
            _ => None,
        },
        _ => None,
    }
}

impl Plot {
    pub(super) fn compiling_redpiler(&self) -> bool {
        self.compilation.as_ref().is_some_and(|job| !job.monitor.cancelled())
    }

    pub(super) fn cancel_redpiler_compilation(&mut self) {
        if let Some(job) = &self.compilation {
            job.monitor.cancel();
        }
    }

    pub(super) fn start_redpiler(&mut self, options: CompilerOptions) {
        // Keep cancelled workers until they finish so repeated requests cannot multiply large jobs.
        if self.compilation.is_some() || self.analysis.is_some() {
            for player in &self.players {
                player.send_error_message(messages::REDPILER_COMPILATION_BUSY);
            }
            return;
        }
        let started = Instant::now();
        let entry = EntryVersion::capture(&self.world);
        let mut world = self.world.activation_snapshot();
        let open_barrels: HashSet<_> = self
            .players
            .iter()
            .filter_map(|player| player.open_container.as_ref())
            .filter(|container| container.ty == mchprs_blocks::block_entities::ContainerType::Barrel)
            .map(|container| container.pos)
            .collect();
        let snapshot_ms = started.elapsed().as_millis();
        let mode = super::scoreboard::optimization_mode_label(&options);
        let monitor = Arc::new(TaskMonitor::default());
        let worker_monitor = monitor.clone();
        let span = tracing::Span::current();
        info!(plot_x = self.world.x, plot_z = self.world.z, snapshot_ms, ?options, "Redpiler started");
        self.scoreboard.set_redpiler_state(&self.players, RedpilerState::Compiling);
        self.scoreboard.set_redpiler_options(&self.players, &options);
        let worker = thread::Builder::new()
            .name(format!("compile-{},{}", self.world.x, self.world.z))
            .spawn(move || {
                let _entered = span.enter();
                // Container notifications and their scheduled work belong to the same isolated input.
                for pos in open_barrels {
                    crate::container::set_barrel_open(&mut world, pos, false);
                }
                let ticks = world.to_be_ticked.iter_entries().collect();
                let mut compiler = Compiler::default();
                let result = compiler.compile(&world, world.get_corners(), options, ticks, worker_monitor);
                CompilationResult { world, compiler, result }
            });
        match worker {
            Ok(worker) => {
                self.compilation = Some(CompilationJob {
                    started,
                    monitor,
                    entry,
                    mode,
                    worker,
                })
            }
            Err(error) => self.reject_redpiler(started, &format!("cannot start compiler worker: {error}")),
        }
        self.reset_timings();
    }

    pub(super) fn start_redpiler_analysis(&mut self, player: usize, options: CompilerOptions, graph_only: bool) {
        if self.compilation.is_some() || self.analysis.is_some() {
            self.players[player].send_error_message(messages::REDPILER_COMPILATION_BUSY);
            return;
        }
        if ANALYSIS_WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < MAX_ANALYSIS_WORKERS).then_some(n + 1))
            .is_err()
        {
            self.players[player].send_error_message(messages::REDPILER_COMPILATION_BUSY);
            return;
        }
        let permit = AnalysisWorkerPermit;
        let started = Instant::now();
        let world = self.world.activation_snapshot();
        let player = self.players[player].entity_id;
        let monitor = Arc::new(TaskMonitor::default());
        let worker_monitor = monitor.clone();
        let span = tracing::Span::current();
        let worker = thread::Builder::new()
            .name(format!("analyze-{},{}", self.world.x, self.world.z))
            .spawn(move || {
                let _permit = permit;
                let _entered = span.enter();
                let ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
                let mode = super::scoreboard::optimization_mode_label(&options);
                if graph_only {
                    let candidate = match crate::redpiler::analysis::graph::prepare_candidate_graph(
                        &world,
                        world.get_corners(),
                        &ticks,
                        &options,
                        worker_monitor.clone(),
                    ) {
                        Ok(candidate) => candidate,
                        Err(error) => return Err(error.to_string()),
                    };
                    let summary = candidate.summary();
                    let mut lines = vec![format!(
                        "Prepared Graph: {mode}; {} nodes, {} links; no compiler was activated",
                        summary.nodes, summary.links
                    )];
                    lines.extend(worker_monitor.graph_statistics().chat_summary_lines());
                    debug!(report = %serde_json::to_string(&candidate.report).unwrap(), graph = ?candidate.graph, "Redpiler candidate graph");
                    Ok(AnalysisOutput { lines, error: None })
                } else {
                    let report = match crate::redpiler::analysis::analyze(
                        &world,
                        world.get_corners(),
                        &ticks,
                        &worker_monitor,
                        crate::redpiler::analysis::AnalysisLimits::for_budget(options.budget_multiplier),
                    ) {
                        Ok(report) => report,
                        Err(error) => return Err(error.to_string()),
                    };
                    let mut lines = vec![report.summary()];
                    debug!(report = %serde_json::to_string(&report).unwrap(), "Redpiler analysis");
                    let mut compiler = Compiler::default();
                    let error = compiler.compile(&world, world.get_corners(), options, ticks, worker_monitor).err();
                    let error = error.map(|error| format!("Redpiler: {error}"));
                    if error.is_none() {
                        if let Some(stats) = compiler.stats() {
                            lines.extend(stats.chat_summary_lines(&mode));
                        }
                        lines.push(format!("This plot can compile in {mode} mode."));
                    }
                    Ok(AnalysisOutput { lines, error })
                }
            });
        match worker {
            Ok(worker) => {
                self.analysis = Some(AnalysisJob {
                    started,
                    player,
                    monitor,
                    worker,
                })
            }
            Err(error) => {
                self.players
                    .iter()
                    .find(|p| p.entity_id == player)
                    .map(|p| p.send_error_message(&format!("Could not start Redpiler analysis: {error}")));
            }
        }
    }

    pub(super) fn cancel_redpiler_analysis(&mut self) {
        if let Some(job) = &self.analysis {
            job.monitor.cancel();
        }
    }

    pub(super) fn poll_redpiler_analysis(&mut self) {
        if !self.analysis.as_ref().is_some_and(|job| job.worker.is_finished()) {
            return;
        }
        let job = self.analysis.take().unwrap();
        let output = job.worker.join();
        if job.monitor.cancelled() {
            return;
        }
        let Some(player) = self.players.iter().find(|player| player.entity_id == job.player) else {
            return;
        };
        match output {
            Ok(Ok(output)) => {
                for line in output.lines {
                    player.send_system_message(&line);
                }
                if let Some(error) = output.error {
                    player.send_error_message(&error);
                }
            }
            Ok(Err(error)) => player.send_error_message(&error),
            Err(_) => player.send_error_message("Redpiler analysis worker failed."),
        }
        debug!(elapsed_ms = job.started.elapsed().as_millis(), "Redpiler analysis completed");
    }

    pub(super) fn poll_redpiler_compilation(&mut self) {
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
                self.reject_redpiler(job.started, "compiler worker failed");
                return;
            }
        };
        if let Err(error) = output.result {
            if let Some(piston) = rejected_piston(&error, &output.world) {
                for player in 0..self.players.len() {
                    self.show_block_highlight(player, piston, false);
                }
            }
            self.reject_redpiler(job.started, &error.to_string());
        } else if !job.entry.matches(&self.world) || self.git_checkout_locked() {
            self.reject_redpiler(job.started, messages::REDPILER_COMPILATION_CHANGED);
        } else {
            self.redpiler = output.compiler;
            if let Some(stats) = self.redpiler.stats() {
                for line in stats.chat_summary_lines(&job.mode) {
                    for player in &self.players {
                        player.send_system_message(&line);
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
            self.scoreboard.set_redpiler_state(&self.players, RedpilerState::Running);
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
            player.send_error_message(&format!("Redpiler: {reason}. Use /rp analyze for details."));
        }
        self.scoreboard.set_redpiler_state(&self.players, RedpilerState::Stopped);
        self.scoreboard.set_redpiler_options(&self.players, &Default::default());
        self.reset_timings();
    }
}
