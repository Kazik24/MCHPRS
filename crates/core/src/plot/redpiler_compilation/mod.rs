//! Compile an isolated snapshot while the plot continues handling players and commands.
use super::*;
use crate::redpiler::{CompileError, TaskMonitor};
use std::sync::Arc;
use std::thread::JoinHandle;

#[cfg(test)]
mod tests;


pub(super) struct CompilationJob {
    /// Time the request started, including snapshot preparation.
    started: Instant,
    /// Cooperative cancellation shared with the compiler worker.
    monitor: Arc<TaskMonitor>,
    /// Live entry state that must still match before activation.
    entry: EntryVersion,
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
    /// All plot chunks also cover the compiler's read-only probe context.
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

fn rejected_position(error: &CompileError) -> Option<BlockPos> {
    match error {
        CompileError::Unsupported(error) => error.position,
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
                    worker,
                })
            }
            Err(error) => self.reject_redpiler(started, &format!("cannot start compiler worker: {error}")),
        }
        self.reset_timings();
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
            if let Some(position) = rejected_position(&error) {
                for player in 0..self.players.len() {
                    self.show_block_highlight(player, position, false);
                }
            }
            self.reject_redpiler(job.started, &error.to_string());
        } else if !job.entry.matches(&self.world) || self.git_checkout_locked() {
            self.reject_redpiler(job.started, messages::REDPILER_COMPILATION_CHANGED);
        } else {
            self.redpiler = output.compiler;
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
            player.send_error_message(&format!("Redpiler: {reason}."));
        }
        self.scoreboard.set_redpiler_state(&self.players, RedpilerState::Stopped);
        self.scoreboard.set_redpiler_options(&self.players, &Default::default());
        self.reset_timings();
    }
}
