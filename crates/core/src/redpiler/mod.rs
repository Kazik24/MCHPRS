//! The graph backend executes the lowered general redstone program.

pub mod analysis;
pub(crate) mod backend;
mod components;
mod options;
mod passes;
mod power_rules;
pub(crate) mod program_graph;
mod statistics;
mod task_monitor;

use crate::redstone;
use crate::world::{World, for_each_block_mut_optimized};
use backend::graph::GraphBackend;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

pub use backend::graph::GraphExecutionError;
pub use backend::{Queues, TickScheduler};
pub use options::CompilerOptions;
#[cfg(test)]
pub(crate) use options::MAX_OPTIMIZATION_LEVEL;
pub use statistics::{
    CompileStatistics, GraphCounts, GraphExecutionCoverage, GraphStatistics, PassStatistics, PreparationTableStatistics,
};
pub use task_monitor::TaskMonitor;

#[derive(Debug)]
/// Failure to prepare or activate a compiled redstone backend.
pub enum CompileError {
    AlreadyActive,
    Analysis(analysis::AnalysisError),
    Unsupported(Box<analysis::AnalysisReport>),
    Cancelled,
    /// Graph preparation rejected the selection; nothing was published.
    RichGraph(RichGraphError),
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
        }
    }
}

impl std::error::Error for CompileError {}

fn graph_preparation_error(error: passes::rich::PreparationError, monitor: &TaskMonitor) -> CompileError {
    let error = match error {
        passes::rich::PreparationError::Analysis(error) => backend::graph::GraphBackendError::Analysis(error),
        passes::rich::PreparationError::Geometry(error) => backend::graph::GraphBackendError::Geometry(error),
        passes::rich::PreparationError::Electrical(error) => backend::graph::GraphBackendError::Electrical(error),
        passes::rich::PreparationError::Callbacks(error) => backend::graph::GraphBackendError::Callbacks(error),
    };
    graph_backend_error(error, monitor)
}

fn graph_pipeline_error(error: passes::PassPipelineError, monitor: &TaskMonitor) -> CompileError {
    graph_backend_error(backend::graph::GraphBackendError::PassPipeline(error), monitor)
}

fn graph_backend_error(error: backend::graph::GraphBackendError, monitor: &TaskMonitor) -> CompileError {
    if monitor.cancelled() {
        CompileError::Cancelled
    } else {
        CompileError::RichGraph(RichGraphError(error))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// The execution engine selected for a compiled circuit.
pub enum BackendKind {
    /// Compiled redstone execution uses the lowered Graph program.
    Graph,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Cumulative work counters for an active graph backend.
pub struct GraphExecutionCounts {
    pub classic_net_invocations: u64,
    pub classic_receiver_ops: u64,
    pub ticks: u64,
    pub scheduled_ticks: u64,
    pub callbacks: u64,
    pub wire_walks: u64,
    /// Directional power samples omitted by certified dust-path queries.
    pub dust_path_power_samples_elided: u64,
    pub piston_samples: u64,
    pub piston_actions: u64,
    /// Piston-driven block changes waiting for publication to the world.
    pub deferred_publications: u64,
}

#[derive(Default)]
/// Owns the active compiled backend and its options and statistics.
pub struct Compiler {
    backend: Option<Box<GraphBackend>>,
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
        self.backend.as_ref().map(|_| BackendKind::Graph)
    }

    /// Executed graph work; None unless the active backend is the graph.
    pub fn graph_execution_counts(&self) -> Option<GraphExecutionCounts> {
        Some(self.backend.as_ref()?.execution_counts())
    }

    /// Current structural ownership, including classic nets that deoptimized after compilation.
    pub fn graph_execution_coverage(&self) -> Option<GraphExecutionCoverage> {
        Some(self.backend.as_ref()?.execution_coverage())
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
        let (backend, analysis_duration) = Self::prepare_graph_backend(world, bounds, ticks, &options, &monitor, graph_pipeline)?;
        let backend_duration = backend_start.elapsed();
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        monitor.inc_progress();

        let statistics = CompileStatistics {
            graph: monitor.graph_statistics(),
            backend: BackendKind::Graph,
            backend_nodes: backend.node_count(),
            execution_coverage: backend.execution_coverage(),
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

    fn prepare_graph_backend<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: &TaskMonitor,
        graph_pipeline: Option<passes::PassPipeline>,
    ) -> Result<(Box<GraphBackend>, Duration), CompileError> {
        let limits = analysis::AnalysisLimits::for_budget(monitor.budget_multiplier());
        let mut prepared = passes::rich::prepare(world, bounds, ticks, monitor, limits).map_err(|error| graph_preparation_error(error, monitor))?;
        let analysis_duration = prepared.analysis_duration;
        let pipeline = graph_pipeline.unwrap_or_else(|| passes::default_graph_pipeline(options));
        passes::run_graph_pipeline(&mut prepared.graph, options, &prepared.entry, &pipeline, monitor)
            .map_err(|error| graph_pipeline_error(error, monitor))?;
        prepared.finish_pipeline_statistics(monitor);
        let lowered = prepared
            .lower(monitor)
            .map_err(|error| graph_backend_error(backend::graph::GraphBackendError::Graph(error), monitor))?;
        let backend = backend::graph::GraphBackend::from_lowered(lowered, monitor).map_err(|error| graph_backend_error(error, monitor))?;
        Ok((Box::new(backend), analysis_duration))
    }

    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        if let Some(mut backend) = self.backend.take() {
            backend.reset(world);
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

    fn backend(&mut self) -> &mut GraphBackend {
        self.backend.as_mut().expect("tried to get redpiler backend when inactive")
    }

    pub fn tick(&mut self) -> Result<(), GraphExecutionError> {
        self.backend().tick()
    }

    pub fn on_use_block(&mut self, pos: BlockPos) {
        if let Err(error) = self.backend().use_block(pos) {
            tracing::error!(%error, "graph backend control failed");
        }
    }

    pub fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        if let Err(error) = self.backend().set_pressure_plate(pos, powered) {
            tracing::error!(%error, "graph backend control failed");
        }
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

/// Inputs needed to compile a bounded region of a world.
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
                        Block::PistonHead { head: piston.into() }
                    } else {
                        Block::Air
                    },
                );
                for optimize in [false, true] {
                    let options = CompilerOptions {
                        optimization_level: if optimize { None } else { Some(0) },
                        ..Default::default()
                    };
                    let mut compiler = Compiler::default();
                    compiler.compile(&world, bounds, options, vec![], Arc::default()).unwrap();
                    assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
                    assert_eq!(compiler.stats().unwrap().backend, BackendKind::Graph);
                    compiler.tick().unwrap();
                    assert_eq!(compiler.graph_execution_counts().unwrap().ticks, 1);
                    compiler.reset(&mut world, bounds);
                    assert_eq!(world.get_block(pos), Block::Piston { piston });
                    let candidate =
                        analysis::graph::prepare_candidate_graph(&world, bounds, &[], &CompilerOptions::default(), Arc::default()).unwrap();
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
            let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
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
            .compile(&scratch, bounds, CompilerOptions::default(), ticks, Arc::default())
            .unwrap();
        assert_eq!(compiler.backend_kind(), Some(BackendKind::Graph));
        let published = compiler.stats().unwrap().total_duration;
        compiler.reset(&mut scratch, bounds);

        let cancelled = Arc::new(TaskMonitor::default());
        cancelled.cancel();
        let cases: [(&str, &PlotWorld, CompilerOptions, Arc<TaskMonitor>); 2] = [
            ("moved observer", &moved_observer, CompilerOptions::default(), Arc::default()),
            ("cancelled", &world, CompilerOptions::default(), cancelled),
        ];
        for (name, input, options, monitor) in cases {
            let before = interpreter_snapshot(input, bounds);
            let ticks = input.scheduler().iter_entries().collect();
            let error = compiler.compile(input, bounds, options, ticks, monitor).unwrap_err();
            eprintln!("{name}: {error}");
            match name {
                "moved observer" => assert!(matches!(error, CompileError::RichGraph(_))),
                "cancelled" => assert!(matches!(
                    error,
                    CompileError::Cancelled | CompileError::Analysis(analysis::AnalysisError::Cancelled)
                )),
                _ => unreachable!(),
            }
            assert!(!compiler.is_active(), "{name}");
            assert!(compiler.current_flags().is_none(), "{name}");
            assert_eq!(compiler.stats().unwrap().total_duration, published, "{name}");
            assert_eq!(interpreter_snapshot(input, bounds), before, "{name}");
        }
    }

    #[test]
    fn parse_options() {
        let input = "-iO0 -U";
        let expected_options = CompilerOptions {
            classic_redstone: false,
            budget_multiplier: 0,
            io_only: true,
            optimization_level: Some(0),
            update: true,
            stable_instant_inputs: false,
        };
        let options = CompilerOptions::parse(input).unwrap();

        assert_eq!(options, expected_options);
    }

    #[test]
    fn optimization_flags_share_the_same_level_validation() {
        for prefix in ["-O", "-o", "-iO", "-uo"] {
            for level in 0..=MAX_OPTIMIZATION_LEVEL {
                let options = CompilerOptions::parse(&format!("{prefix}{level}")).unwrap();
                assert_eq!(options.optimization_level, Some(level));
            }
            assert_eq!(CompilerOptions::parse(prefix).unwrap().optimization_level, None);
            assert!(CompilerOptions::parse(&format!("{prefix}{}", MAX_OPTIMIZATION_LEVEL + 1)).is_err());
        }
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
        for flags in ["--unknown", "--export", "--export-dot", "-ox", "-"] {
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
            .compile(&world, bounds, CompilerOptions::default(), Vec::new(), Arc::default())
            .unwrap();
        let statistics = compiler.stats().unwrap();
        assert!(!statistics.graph.passes.is_empty());
        assert!(statistics.graph.transformations.iter().all(|pass| !pass.enabled));
        assert_eq!(statistics.graph.baseline(), statistics.graph.final_graph());
        assert!(statistics.backend_nodes > 0);
        let total_duration = statistics.total_duration;
        compiler.reset(&mut world, bounds);
        let cancelled = Arc::new(TaskMonitor::default());
        cancelled.cancel();
        assert!(
            compiler
                .compile(&world, bounds, CompilerOptions::default(), Vec::new(), cancelled)
                .is_err()
        );
        assert_eq!(compiler.stats().unwrap().total_duration, total_duration);
    }
}
