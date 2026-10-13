//! The graph backend executes the lowered general redstone program.

pub mod analysis;
pub(crate) mod backend;
mod components;
mod passes;
pub(crate) mod program_graph;
mod task_monitor;

use crate::redstone;
use crate::world::{World, for_each_block_mut_optimized};
use backend::Runtime;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

pub use backend::graph::GraphExecutionError;
pub use backend::{Queues, TickScheduler};
pub use task_monitor::TaskMonitor;

#[derive(Debug)]
pub enum CompileError {
    AlreadyActive,
    Analysis(analysis::AnalysisError),
    Unsupported(Box<analysis::AnalysisReport>),
    Cancelled,
    /// Graph preparation rejected the selection; nothing was published.
    RichGraph(RichGraphError),
    /// Legacy binary and dot exports cannot carry piston, contact, or route edges.
    UnsupportedRichExport,
}

/// Structured rich-graph preparation failure, kept opaque outside the crate.
#[derive(Debug)]
pub struct RichGraphError(pub(crate) backend::graph::GraphBackendError);

impl std::fmt::Display for RichGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActive => f.write_str("reset the active compiler before recompiling"),
            Self::Analysis(error) => error.fmt(f),
            Self::Unsupported(report) => {
                write!(f, "{}", report.summary())?;
                if let Some(issue) = report.issues.first() {
                    write!(f, "; {issue}")?;
                }
                Ok(())
            }
            Self::Cancelled => f.write_str("compilation cancelled"),
            Self::RichGraph(error) => write!(f, "graph preparation rejected: {error}"),
            Self::UnsupportedRichExport => f.write_str(
                "legacy binary and DOT exports cannot represent the unified graph; compile without --export or --export-dot",
            ),
        }
    }
}

impl std::error::Error for CompileError {}

fn graph_preparation_error(
    error: passes::rich::PreparationError,
    monitor: &TaskMonitor,
) -> CompileError {
    let error = match error {
        passes::rich::PreparationError::Analysis(error) => {
            backend::graph::GraphBackendError::Analysis(error)
        }
        passes::rich::PreparationError::Geometry(error) => {
            backend::graph::GraphBackendError::Geometry(error)
        }
        passes::rich::PreparationError::Electrical(error) => {
            backend::graph::GraphBackendError::Electrical(error)
        }
        passes::rich::PreparationError::Callbacks(error) => {
            backend::graph::GraphBackendError::Callbacks(error)
        }
    };
    graph_backend_error(error, monitor)
}

fn graph_pipeline_error(error: passes::PassPipelineError, monitor: &TaskMonitor) -> CompileError {
    graph_backend_error(
        backend::graph::GraphBackendError::PassPipeline(error),
        monitor,
    )
}

fn graph_backend_error(
    error: backend::graph::GraphBackendError,
    monitor: &TaskMonitor,
) -> CompileError {
    if monitor.cancelled() {
        CompileError::Cancelled
    } else {
        CompileError::RichGraph(RichGraphError(error))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// Compiled redstone execution uses the lowered Graph program.
    Graph,
}

/// Work performed by the graph backend since activation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphExecutionCounts {
    pub ticks: u64,
    pub scheduled_ticks: u64,
    pub callbacks: u64,
    pub wire_walks: u64,
    pub piston_samples: u64,
    pub piston_actions: u64,
    pub deferred_publications: u64,
}

/// One inclusive timing bucket from an opt-in `graph-profile` benchmark build.
#[cfg(feature = "graph-profile")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphProfileMetric {
    /// Number of calls included in this bucket.
    pub calls: u64,
    /// Inclusive elapsed time across those calls, in nanoseconds.
    pub elapsed_ns: u128,
}

/// Inclusive Graph timings; nested buckets overlap and must not be summed.
#[cfg(feature = "graph-profile")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphExecutionProfile {
    /// Time in scheduler and piston phases, excluding command-host preflight.
    pub tick_phases: GraphProfileMetric,
    /// Time draining scheduled work before and after scheduler advancement.
    pub scheduled_drain: GraphProfileMetric,
    /// Time validating and accepting queued piston actions.
    pub piston_actions: GraphProfileMetric,
    /// Time publishing deferred piston connections.
    pub deferred_connections: GraphProfileMetric,
    /// Time delivering compiled callback routes, including nested callbacks.
    pub route_delivery: GraphProfileMetric,
    /// Time walking a Turbo wire neighborhood, including nested deliveries.
    pub turbo_wire_walk: GraphProfileMetric,
    /// Time sampling piston inputs and checking their compiled lane.
    pub piston_sample: GraphProfileMetric,
}

#[derive(Default, PartialEq, Eq, Debug)]
pub struct CompilerOptions {
    /// Set by the server from the initiating player's rank, never from flags.
    /// Zero retains the default 1x budget; values are capped at 8x.
    pub budget_multiplier: usize,
    /// Enable configured graph optimization passes.
    pub optimize: bool,
    /// Request the legacy binary export, which cannot represent the unified graph.
    pub export: bool,
    /// Only flush visible outputs and controls, including lamps and copper bulbs.
    pub io_only: bool,
    /// Update all blocks in the input region after reset.
    pub update: bool,
    /// Request the legacy DOT export, which cannot represent the unified graph.
    pub export_dot_graph: bool,
}

impl CompilerOptions {
    pub fn parse(flags: &str) -> Result<CompilerOptions, String> {
        let mut options = Self::default();
        for option in flags.split_whitespace() {
            if option.starts_with("--") {
                match option {
                    "--optimize" => options.optimize = true,
                    "--export" => options.export = true,
                    "--io-only" => options.io_only = true,
                    "--update" => options.update = true,
                    "--export-dot" => options.export_dot_graph = true,
                    _ => return Err(format!("Unrecognized Redpiler option: {option}")),
                }
            } else if let Some(short_flags) = option.strip_prefix('-') {
                if short_flags.is_empty() {
                    return Err(format!("Unrecognized Redpiler option: {option}"));
                }
                for c in short_flags.chars() {
                    match c.to_ascii_lowercase() {
                        'o' => options.optimize = true,
                        'e' => options.export = true,
                        'i' => options.io_only = true,
                        'u' => options.update = true,
                        _ => return Err(format!("Unrecognized Redpiler option: -{c}")),
                    }
                }
            } else {
                return Err(format!("Unrecognized Redpiler option: {option}"));
            }
        }
        Ok(options)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphCounts {
    pub nodes: usize,
    pub links: usize,
}

impl GraphCounts {
    pub(crate) fn of(graph: &program_graph::CompileGraph) -> Self {
        Self {
            nodes: graph.nodes.len(),
            links: graph.links.len() + graph.power_links.len() + graph.wire_relations.len(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PassStatistics {
    pub name: &'static str,
    pub enabled: bool,
    pub before: GraphCounts,
    pub after: GraphCounts,
    pub duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparationTableStatistics {
    pub stage: &'static str,
    pub table: &'static str,
    pub entries: usize,
    pub capacity: usize,
    /// Estimates retained element storage and known nested slices, excluding maps and allocator overhead.
    pub estimated_bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct GraphStatistics {
    pub passes: Vec<PassStatistics>,
    /// This contains only optional transformations, not mandatory parser stages.
    pub transformations: Vec<PassStatistics>,
    /// Retained table capacities sampled after each required preparation stage.
    pub preparation_tables: Vec<PreparationTableStatistics>,
    /// This records the graph after required parsing and before transformations.
    pub parser_complete: Option<GraphCounts>,
    /// This records the final graph independently of the supplied pass count.
    pub final_counts: Option<GraphCounts>,
    pub duration: Duration,
}

impl GraphStatistics {
    pub fn baseline(&self) -> Option<GraphCounts> {
        self.parser_complete
    }

    pub fn final_graph(&self) -> Option<GraphCounts> {
        self.final_counts
    }

    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let (Some(before), Some(after)) = (self.baseline(), self.final_graph()) {
            lines.push(format!(
                "Graph after required preparation: {} nodes / {} links; after passes: {} nodes / {} links (change {:+} nodes / {:+} links)",
                before.nodes, before.links, after.nodes, after.links,
                after.nodes as i128 - before.nodes as i128,
                after.links as i128 - before.links as i128,
            ));
            lines.push("Construction: all parsed wire nodes are retained.".into());
        }
        if let Some(stage) = self.preparation_tables.last().map(|table| table.stage) {
            let mut tables: Vec<_> = self
                .preparation_tables
                .iter()
                .filter(|table| table.stage == stage)
                .collect();
            tables.sort_by_key(|table| std::cmp::Reverse(table.estimated_bytes));
            let estimate = tables
                .iter()
                .map(|table| table.estimated_bytes)
                .sum::<usize>();
            let largest = tables
                .iter()
                .take(3)
                .map(|table| {
                    format!(
                        "{} {} entries, capacity {}, {:.1} MiB",
                        table.table,
                        table.entries,
                        table.capacity,
                        table.estimated_bytes as f64 / (1024.0 * 1024.0),
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!(
                "Graph preparation table estimate at {stage}: {:.1} MiB retained capacity; largest: {largest}",
                estimate as f64 / (1024.0 * 1024.0),
            ));
        }
        let enabled = self
            .transformations
            .iter()
            .filter(|pass| pass.enabled)
            .count();
        let skipped = self.transformations.len() - enabled;
        lines.push(if self.transformations.is_empty() {
            "Transformations: none configured.".into()
        } else {
            format!("Transformations: {enabled} enabled, {skipped} skipped.")
        });
        lines.extend(self.passes.iter().map(|pass| {
            format!(
                "{} [{}]: {} -> {} nodes, {} -> {} links, {:.3} ms",
                pass.name,
                if pass.enabled { "enabled" } else { "skipped" },
                pass.before.nodes,
                pass.after.nodes,
                pass.before.links,
                pass.after.links,
                pass.duration.as_secs_f64() * 1000.0,
            )
        }));
        lines
    }
}

#[derive(Clone, Debug)]
pub struct CompileStatistics {
    pub graph: GraphStatistics,
    pub backend: BackendKind,
    pub backend_nodes: usize,
    pub analysis_duration: Duration,
    pub backend_duration: Duration,
    pub total_duration: Duration,
}

impl CompileStatistics {
    pub fn summary_lines(&self) -> Vec<String> {
        let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
        let mut lines = vec![format!(
            "Compile: {:.3} ms total; analysis {:.3}, graph {:.3}, backend {:.3} ms; {} backend nodes",
            ms(self.total_duration),
            ms(self.analysis_duration),
            ms(self.graph.duration),
            ms(self.backend_duration),
            self.backend_nodes,
        )];
        if self.backend == BackendKind::Graph {
            lines.push(
                "Execution: unified Graph program; no built-in optimization passes are configured, so -O has no effect.".into(),
            );
        }
        lines.extend(self.graph.summary_lines().into_iter().take(3));
        lines
    }
}

#[derive(Default)]
pub struct Compiler {
    backend: Option<Runtime>,
    options: CompilerOptions,
    statistics: Option<CompileStatistics>,
}

impl Compiler {
    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(BlockPos, u8)> {
        self.backend.as_ref().unwrap().ordinary_sources()
    }

    /// Last successful compile; failed attempts never publish partial statistics.
    pub fn stats(&self) -> Option<&CompileStatistics> {
        self.statistics.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.backend.is_some()
    }

    pub fn current_flags(&self) -> Option<&CompilerOptions> {
        self.backend.as_ref().map(|_| &self.options)
    }

    pub fn backend_kind(&self) -> Option<BackendKind> {
        self.backend.as_ref().map(Runtime::kind)
    }

    /// Executed graph work; None unless the active backend is the graph.
    pub fn graph_execution_counts(&self) -> Option<GraphExecutionCounts> {
        self.backend.as_ref()?.graph_execution_counts()
    }

    /// Inclusive operation timings, available only in an opt-in profiling build.
    #[cfg(feature = "graph-profile")]
    pub fn graph_execution_profile(&self) -> Option<GraphExecutionProfile> {
        self.backend.as_ref()?.graph_execution_profile()
    }

    pub fn compile<W: World>(
        &mut self,
        world: &W,
        bounds: (BlockPos, BlockPos),
        options: CompilerOptions,
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
    ) -> Result<(), CompileError> {
        self.compile_with_graph_pipeline(world, bounds, options, ticks, monitor, None)
    }

    fn compile_with_graph_pipeline<W: World>(
        &mut self,
        world: &W,
        bounds: (BlockPos, BlockPos),
        options: CompilerOptions,
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
        graph_pipeline: Option<passes::PassPipeline>,
    ) -> Result<(), CompileError> {
        let span = tracing::info_span!("redpiler", ?bounds, ?options);
        let _entered = span.enter();
        tracing::info!("Compiler started");
        debug!("Starting compile");
        let start = Instant::now();

        if self.is_active() {
            return Err(CompileError::AlreadyActive);
        }
        monitor.clear_graph_statistics();
        monitor.set_budget_multiplier(options.budget_multiplier);
        let backend_start = Instant::now();
        let (backend, analysis_duration) =
            Self::prepare_graph_backend(world, bounds, ticks, &options, &monitor, graph_pipeline)?;
        self.publish(
            backend,
            options,
            &monitor,
            start,
            analysis_duration,
            backend_start,
        )
    }

    fn prepare_graph_backend<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: &TaskMonitor,
        graph_pipeline: Option<passes::PassPipeline>,
    ) -> Result<(Runtime, Duration), CompileError> {
        if options.export || options.export_dot_graph {
            return Err(CompileError::UnsupportedRichExport);
        }
        let limits = analysis::AnalysisLimits::for_budget(monitor.budget_multiplier());
        let mut prepared = passes::rich::prepare(world, bounds, ticks, monitor, limits)
            .map_err(|error| graph_preparation_error(error, monitor))?;
        let analysis_duration = prepared.analysis_duration;
        let pipeline = graph_pipeline.unwrap_or_else(|| passes::default_graph_pipeline(options));
        passes::run_graph_pipeline(
            &mut prepared.graph,
            options,
            &prepared.entry,
            &pipeline,
            monitor,
        )
        .map_err(|error| graph_pipeline_error(error, monitor))?;
        prepared.finish_pipeline_statistics(monitor);
        let lowered = prepared.lower(&monitor).map_err(|error| {
            graph_backend_error(backend::graph::GraphBackendError::Graph(error), monitor)
        })?;
        let backend = backend::graph::GraphBackend::from_lowered(lowered, monitor)
            .map_err(|error| graph_backend_error(error, monitor))?;
        Ok((Runtime::Graph(Box::new(backend)), analysis_duration))
    }

    /// The only place a staged backend becomes active.
    fn publish(
        &mut self,
        backend: Runtime,
        options: CompilerOptions,
        monitor: &TaskMonitor,
        start: Instant,
        analysis_duration: Duration,
        backend_start: Instant,
    ) -> Result<(), CompileError> {
        let backend_duration = backend_start.elapsed();
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        monitor.inc_progress();

        let statistics = CompileStatistics {
            graph: monitor.graph_statistics(),
            backend: backend.kind(),
            backend_nodes: backend.node_count(),
            analysis_duration,
            backend_duration,
            total_duration: start.elapsed(),
        };
        self.backend = Some(backend);
        self.statistics = Some(statistics);
        self.options = options;
        tracing::info!(stats = ?self.statistics, "Compiler completed");
        debug!("Compile completed in {:?}", start.elapsed());
        Ok(())
    }

    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        if let Some(mut backend) = self.backend.take() {
            backend.reset(world, self.options.io_only);
        }

        if self.options.update {
            let (first_pos, second_pos) = bounds;
            for_each_block_mut_optimized(world, first_pos, second_pos, |world, pos| {
                let block = world.get_block(pos);
                redstone::update(block, world, pos, None);
            });
        }
        self.options = Default::default();
    }

    fn backend(&mut self) -> &mut Runtime {
        self.backend
            .as_mut()
            .expect("tried to get redpiler backend when inactive")
    }

    pub fn tick(&mut self) -> Result<(), GraphExecutionError> {
        self.backend().tick()
    }

    pub fn on_use_block(&mut self, pos: BlockPos) {
        self.backend().on_use_block(pos);
    }

    pub fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        self.backend().set_pressure_plate(pos, powered);
    }

    pub fn flush<W: World>(&mut self, world: &mut W) {
        let io_only = self.options.io_only;
        self.backend().flush(world, io_only);
    }

    pub fn tick_with_world<W: World>(&mut self, world: &mut W) -> Result<(), GraphExecutionError> {
        self.backend().tick_with_world(world)
    }

    pub fn inspect(&mut self, pos: BlockPos) {
        if let Some(backend) = &mut self.backend {
            backend.inspect(pos);
        } else {
            debug!("cannot inspect when backend is not running");
        }
    }
}

pub struct CompilerInput<'w, W: World> {
    pub world: &'w W,
    pub bounds: (BlockPos, BlockPos),
    pub ticks: &'w [TickEntry],
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_blocks::blocks::Block;

    #[test]
    fn stationary_pistons_select_the_graph_backend_in_every_compiler_mode() {
        use crate::plot::PlotWorld;
        use crate::world::storage::Chunk;
        use mchprs_blocks::BlockFacing;
        use mchprs_blocks::blocks::RedstonePiston;

        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let pos = BlockPos::new(4, 20, 4);
        let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
        for sticky in [false, true] {
            for extended in [false, true] {
                let piston = RedstonePiston {
                    facing: BlockFacing::South,
                    sticky,
                    extended,
                };
                world.set_block(pos, Block::Piston { piston });
                let head = pos.offset(mchprs_blocks::BlockFace::South);
                world.set_block(
                    head,
                    if extended {
                        Block::PistonHead {
                            head: piston.into(),
                        }
                    } else {
                        Block::Air
                    },
                );
                for optimize in [false, true] {
                    let options = CompilerOptions {
                        optimize,
                        ..Default::default()
                    };
                    let mut compiler = Compiler::default();
                    compiler
                        .compile(&world, bounds, options, vec![], Arc::default())
                        .unwrap();
                    assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
                    assert_eq!(compiler.stats().unwrap().backend, BackendKind::Graph);
                    compiler.tick().unwrap();
                    assert_eq!(compiler.graph_execution_counts().unwrap().ticks, 1);
                    compiler.reset(&mut world, bounds);
                    assert_eq!(world.get_block(pos), Block::Piston { piston });
                    let candidate = analysis::graph::prepare_candidate_graph(
                        &world,
                        bounds,
                        &[],
                        &CompilerOptions::default(),
                        Arc::default(),
                    )
                    .unwrap();
                    assert!(candidate.summary().nodes > 0);
                }
            }
        }
    }

    /// Everything activation could disturb: blocks, entities, ordinary ticks,
    /// and the interpreter's logical piston state.
    fn interpreter_snapshot(
        world: &crate::plot::PlotWorld,
        bounds: (BlockPos, BlockPos),
    ) -> (Vec<(BlockPos, u32, Option<String>)>, Vec<TickEntry>, String) {
        let mut cells = Vec::new();
        for y in bounds.0.y..=bounds.1.y {
            for z in bounds.0.z..=bounds.1.z {
                for x in bounds.0.x..=bounds.1.x {
                    let pos = BlockPos::new(x, y, z);
                    let entity = world.get_block_entity(pos).map(|e| format!("{e:?}"));
                    let raw = world.get_block_raw(pos);
                    if raw != 0 || entity.is_some() {
                        cells.push((pos, raw, entity));
                    }
                }
            }
        }
        (
            cells,
            world.scheduler().iter_entries().collect(),
            serde_json::to_string(world.piston_state()).unwrap(),
        )
    }

    #[test]
    fn failed_and_cancelled_piston_compilation_preserve_interpreter_state() {
        use crate::plot::PlotWorld;
        use crate::world::storage::Chunk;
        use mchprs_blocks::BlockFace;
        use mchprs_blocks::blocks::{RedstonePiston, RedstoneRepeater};
        use mchprs_world::TickPriority;

        let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
        let base = BlockPos::new(4, 20, 4);
        let repeater = BlockPos::new(10, 20, 10);
        let build = |payload: Block| {
            let mut world =
                PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
            world.set_block(
                base,
                Block::Piston {
                    piston: RedstonePiston {
                        facing: mchprs_blocks::BlockFacing::East,
                        sticky: true,
                        extended: false,
                    },
                },
            );
            world.set_block(base.offset(BlockFace::East), payload);
            world.set_block(repeater.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                repeater,
                Block::RedstoneRepeater {
                    repeater: RedstoneRepeater::default(),
                },
            );
            world.schedule_tick(repeater, 1, TickPriority::High);
            world.piston_state_mut().logical_tick = 77;
            world
        };
        let world = build(Block::RedstoneBlock);
        let moved_observer = build(Block::Observer {
            observer: Default::default(),
        });

        // A previous success publishes statistics that failures must keep.
        let mut compiler = Compiler::default();
        let mut scratch = build(Block::RedstoneBlock);
        let ticks = scratch.scheduler().iter_entries().collect();
        compiler
            .compile(
                &scratch,
                bounds,
                CompilerOptions::default(),
                ticks,
                Arc::default(),
            )
            .unwrap();
        assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
        let published = compiler.stats().unwrap().total_duration;
        compiler.reset(&mut scratch, bounds);

        let cancelled = Arc::new(TaskMonitor::default());
        cancelled.cancel();
        let cases: [(&str, &PlotWorld, CompilerOptions, Arc<TaskMonitor>); 4] = [
            (
                "moved observer",
                &moved_observer,
                CompilerOptions::default(),
                Arc::default(),
            ),
            ("cancelled", &world, CompilerOptions::default(), cancelled),
            (
                "binary export",
                &world,
                CompilerOptions {
                    export: true,
                    ..Default::default()
                },
                Arc::default(),
            ),
            (
                "dot export",
                &world,
                CompilerOptions {
                    export_dot_graph: true,
                    ..Default::default()
                },
                Arc::default(),
            ),
        ];
        for (name, input, options, monitor) in cases {
            let before = interpreter_snapshot(input, bounds);
            let ticks = input.scheduler().iter_entries().collect();
            let error = compiler
                .compile(input, bounds, options, ticks, monitor)
                .unwrap_err();
            eprintln!("{name}: {error}");
            match name {
                "moved observer" => assert!(matches!(error, CompileError::RichGraph(_))),
                "cancelled" => assert!(matches!(
                    error,
                    CompileError::Cancelled
                        | CompileError::Analysis(analysis::AnalysisError::Cancelled)
                )),
                _ => assert!(matches!(error, CompileError::UnsupportedRichExport)),
            }
            assert!(!compiler.is_active(), "{name}");
            assert!(compiler.current_flags().is_none(), "{name}");
            assert_eq!(
                compiler.stats().unwrap().total_duration,
                published,
                "{name}"
            );
            assert_eq!(interpreter_snapshot(input, bounds), before, "{name}");
        }
    }

    #[test]
    fn parse_options() {
        let input = "-iO -U --export";
        let expected_options = CompilerOptions {
            budget_multiplier: 0,
            io_only: true,
            optimize: true,
            export: true,
            update: true,
            export_dot_graph: false,
        };
        let options = CompilerOptions::parse(input).unwrap();

        assert_eq!(options, expected_options);
    }

    #[test]
    fn compilation_budget_cannot_be_selected_by_command_flags() {
        assert!(CompilerOptions::parse("--budget-multiplier=8").is_err());
        let monitor = TaskMonitor::default();
        assert_eq!(monitor.budget_multiplier(), 1);
        monitor.set_budget_multiplier(usize::MAX);
        assert_eq!(monitor.budget_multiplier(), 8);
    }

    #[test]
    fn unknown_flags_fail() {
        for flags in ["--unknown", "-ox", "-"] {
            assert!(CompilerOptions::parse(flags).is_err(), "{flags}");
        }
    }

    #[test]
    fn compile_statistics_publish_only_after_success() {
        use crate::plot::PlotWorld;
        use crate::world::storage::Chunk;
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(BlockPos::new(4, 30, 4), Block::RedstoneLamp { lit: false });
        let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
        let mut compiler = Compiler::default();
        assert!(compiler.stats().is_none());
        compiler
            .compile(
                &world,
                bounds,
                CompilerOptions::default(),
                Vec::new(),
                Arc::default(),
            )
            .unwrap();
        let statistics = compiler.stats().unwrap();
        assert!(!statistics.graph.passes.is_empty());
        assert!(statistics.graph.transformations.is_empty());
        assert_eq!(statistics.graph.baseline(), statistics.graph.final_graph());
        assert!(statistics.backend_nodes > 0);
        let total_duration = statistics.total_duration;
        compiler.reset(&mut world, bounds);
        let cancelled = Arc::new(TaskMonitor::default());
        cancelled.cancel();
        assert!(
            compiler
                .compile(
                    &world,
                    bounds,
                    CompilerOptions::default(),
                    Vec::new(),
                    cancelled
                )
                .is_err()
        );
        assert_eq!(compiler.stats().unwrap().total_duration, total_duration);
    }
}
