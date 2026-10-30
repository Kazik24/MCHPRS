//! Redpiler runs redstone circuits from a compiled graph. It finds components,
//! power connections, and update routes once, then uses them to run the circuit.
//!
//! Power levels, update order, and pending ticks all affect the result. The graph
//! keeps power reads separate from notifications so it can preserve that order.
//! [`Compiler`] owns the circuit. [`Compiler::compile`] builds it without changing
//! the world and makes it active once compilation succeeds.
//!
//! # Building the graph
//!
//! ```text
//! World selection + pending ticks
//!                |
//!    geometry and support checks
//!                |
//!      electrical dependencies
//!                |
//!      ordered callback routes
//!                |
//!           CircuitDraft
//!                |
//!    rewrites and execution plans
//!                |
//!             lowering
//!                |
//!    CircuitProgram + CircuitState
//!                |
//!           GraphBackend
//! ```
//!
//! [Preparation][preparation] has three stages:
//!
//! - [Geometry][geometry] checks that the selection is supported and records
//!   pistons, movable materials, and the positions they can occupy.
//! - [Electrical discovery][electrical] finds components and the power sources
//!   each input can read.
//! - [Callback construction][callbacks] records update and shape notifications
//!   in the order they should run.
//!
//! These produce a `CircuitDraft`, along with the starting clock and pending ticks.
//!
//! # Nodes and connections
//!
//! The [graph][storage] is stored in tables linked by IDs. A **node** represents
//! a component, wire, piston, or contact. A **site** is a world position the
//! circuit needs to read. Its block properties come from a component, a fixed
//! block profile, or whatever currently occupies a contact. Air and solid blocks
//! can be sites too.
//!
//! There are three main kinds of connection:
//!
//! ```text
//! PowerLink:
//!   source site --[port, face, attenuation, guard]--> input / conductor
//!
//! Route (ordered notifications):
//!   source node --> route
//!                    |
//!                    +-- delivery 0 --> target site
//!                    +-- delivery 1 --> target site
//!                    +-- ...
//!
//! WireRelation (neighbor geometry):
//!   wire node --[step / ascend / descend, guard]--> neighboring site
//! ```
//!
//! A **power link** connects a source site to an input or conductor. The source
//! port says which kind of power to read: stored output, weak power, strong power,
//! or conduction. Component inputs have main, side, and quasi ports. To read an
//! input, the executor takes the strongest allowed contribution after attenuation.
//! Reading power does not send notifications.
//!
//! A **route** is an ordered list of notifications. Each delivery records a
//! target site, receiver, direction, and condition. `LinkSpec` describes the
//! corresponding connection between nodes; the route stores the delivery order,
//! including repeated notifications. Feedback loops are allowed: the executor
//! runs events as they occur, rather than visiting every node in graph order.
//!
//! A **wire relation** records a neighboring site a wire may need to read. Each
//! wire's `WirePlan` also stores neighbor sites and notification routes. The
//! [wire executor][wire] processes changes in ordered layers, keeping intermediate
//! power changes and the state captured during the walk.
//!
//! # Pistons and captured state
//!
//! For supported piston movements, a **contact** is a position whose block can
//! change, and a **resource** is the material being moved. A **guard** is a Boolean
//! condition that selects an occupant or enables a connection. For example:
//!
//! ```text
//! contact at position P
//!   +--[resource is at P]----> material profile
//!   +--[head is present]----> piston-head profile
//!   +--[otherwise]----------> background profile
//! ```
//!
//! The actual alternatives depend on the contact. They are recorded during
//! compilation, so a piston movement selects an existing alternative.
//!
//! A **live** read uses current graph state. A **captured** read uses the receiver
//! and properties saved earlier in a wire walk. If a callback moves a block,
//! later deliveries using that capture still see the saved observation. Other
//! reads, such as conducted power, can remain live during the same operation.
//! [Site resolution][sites] implements these rules.
//!
//! # Running a circuit
//!
//! Consider a simple line, with the repeater facing the lamp:
//!
//! ```text
//! lever ---> dust ---> repeater ---> lamp
//! use        wire walk    delayed      output
//!                         transition
//! ```
//!
//! [`Compiler::on_use_block`] toggles the lever and sends its notifications. The
//! dust reads its inputs and starts a wire walk if its power changes. The repeater
//! then reads its input and may schedule a tick. When that tick runs, it changes
//! the repeater's output and notifies the lamp. The diagram leaves out the extra
//! neighbor notifications along the way.
//!
//! The [backend][execution] holds the program, circuit state, clock, and queues.
//! Each tick runs due component ticks, advances the scheduler and checks again,
//! processes piston actions, then applies deferred piston connections.
//! [`Compiler::tick_with_world`] gives command blocks access to the world;
//! ordinary redstone reads the compiled state.
//!
//! # Optimization and reset
//!
//! The [passes][passes] first rewrite the graph, then build faster execution plans
//! for supported cases. Rewriting the graph discards old plans. The runner checks
//! each enabled pass's result; circuits that don't fit an optimization keep the
//! general execution path. These checks cover graph structure and plan certificates,
//! rather than proving every rewrite equivalent to the original circuit.
//!
//! Lowering splits the graph into `CircuitProgram` (immutable instructions) and
//! `CircuitState` (mutable values), and maps pending ticks to compiled receivers.
//!
//! [`Compiler::flush`] writes display changes and effects to the world. The
//! circuit keeps running, and [`CompilerOptions::io_only`] can hide internal
//! changes. [`Compiler::reset`] writes the full state back and returns the clock
//! and pending work to the interpreter. Reset preserves unfinished transitions;
//! it doesn't run them to completion.
//!
//! [`analysis::graph::prepare_candidate_graph`] uses the same preparation and
//! passes to inspect a circuit without starting it.
//!
//! [preparation]: ../../src/mchprs_core/redpiler/passes/rich.rs.html
//! [geometry]: ../../src/mchprs_core/redpiler/analysis/geometry.rs.html
//! [electrical]: ../../src/mchprs_core/redpiler/analysis/electrical.rs.html
//! [callbacks]: ../../src/mchprs_core/redpiler/analysis/callbacks.rs.html
//! [storage]: ../../src/mchprs_core/redpiler/program_graph.rs.html
//! [wire]: ../../src/mchprs_core/redpiler/backend/graph/wire.rs.html
//! [sites]: ../../src/mchprs_core/redpiler/backend/graph/sites.rs.html
//! [execution]: ../../src/mchprs_core/redpiler/backend/graph.rs.html
//! [passes]: ../../src/mchprs_core/redpiler/passes/mod.rs.html

pub mod analysis;
pub(crate) mod backend;
mod components;
#[allow(dead_code)]
mod event_netlist;
#[allow(dead_code)]
pub(crate) mod event_repeater;
mod options;
mod passes;
#[allow(dead_code)]
mod physical_signal_network;
mod power_rules;
pub(crate) mod program_graph;
mod statistics;
mod task_monitor;
#[cfg(test)]
pub(crate) mod test_fixtures;

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
pub use statistics::{CompileStatistics, GraphCounts, GraphExecutionCoverage, GraphStatistics, PassStatistics, PreparationTableStatistics};
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

enum ActiveBackend {
    Graph(Box<GraphBackend>),
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
///
/// Compile before ticking, changing controls, or flushing; these methods panic
/// when inactive. Reset returns the circuit to the interpreter. See the
/// [module overview](self) for how the graph works.
pub struct Compiler {
    backend: Option<ActiveBackend>,
    options: CompilerOptions,
    statistics: Option<CompileStatistics>,
}

impl Compiler {
    #[cfg(test)]
    pub(crate) fn ordinary_sources(&self) -> Vec<(BlockPos, u8)> {
        match self.backend.as_ref().unwrap() {
            ActiveBackend::Graph(graph) => graph.ordinary_sources(),
        }
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
        self.backend.as_ref().map(|backend| match backend {
            ActiveBackend::Graph(_) => BackendKind::Graph,
        })
    }

    /// Executed graph work; None unless the active backend is the graph.
    pub fn graph_execution_counts(&self) -> Option<GraphExecutionCounts> {
        match self.backend.as_ref()? {
            ActiveBackend::Graph(graph) => Some(graph.execution_counts()),
        }
    }

    /// Current structural ownership, including classic nets that deoptimized after compilation.
    pub fn graph_execution_coverage(&self) -> Option<GraphExecutionCoverage> {
        match self.backend.as_ref()? {
            ActiveBackend::Graph(graph) => Some(graph.execution_coverage()),
        }
    }

    /// Compile the selected region and start it with the supplied pending ticks.
    ///
    /// Reads the world without changing it. On failure or cancellation, no new
    /// backend or statistics are installed. Reset an active compiler before compiling again.
    /// Pending ticks in `ticks` are imported into the compiled graph.
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
        self.backend = Some(ActiveBackend::Graph(backend));
        self.statistics = Some(statistics);
        self.options = options;
        tracing::info!(stats = ?self.statistics, "Compiler completed");
        debug!("Compile completed in {:?}", start.elapsed());
        Ok(())
    }

    /// Build a backend ready to run, leaving activation to the caller.
    fn prepare_graph_backend<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: &TaskMonitor,
        graph_pipeline: Option<passes::PassPipeline>,
    ) -> Result<(Box<GraphBackend>, Duration), CompileError> {
        let limits = analysis::AnalysisLimits::for_budget(monitor.budget_multiplier());
        let prepared = passes::rich::prepare(world, bounds, ticks, monitor, limits).map_err(|error| graph_preparation_error(error, monitor))?;
        let analysis_duration = prepared.analysis_duration;
        let pipeline = graph_pipeline.unwrap_or_else(|| passes::default_graph_pipeline(options));
        let prepared = prepared
            .run_passes(options, &pipeline, monitor)
            .map_err(|error| graph_pipeline_error(error, monitor))?;
        let lowered = prepared
            .lower(monitor)
            .map_err(|error| graph_backend_error(backend::graph::GraphBackendError::Graph(error), monitor))?;
        let backend = backend::graph::GraphBackend::from_lowered(lowered, monitor).map_err(|error| graph_backend_error(error, monitor))?;
        Ok((Box::new(backend), analysis_duration))
    }

    /// Write the circuit back to the world and return pending work to the interpreter.
    ///
    /// Stops compiled execution. Call between ticks. With `update` enabled, also
    /// send block updates throughout `bounds`. Does nothing when already inactive.
    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        if let Err(error) = self.try_reset(world, bounds) {
            tracing::error!(%error, "compiled reset failed");
        }
    }

    /// Stop the graph backend.
    pub fn try_reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) -> Result<(), GraphExecutionError> {
        if let Some(backend) = self.backend.as_mut() {
            match backend {
                ActiveBackend::Graph(graph) => graph.reset(world),
            }
            self.backend = None;
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

    #[cfg(test)]
    fn backend(&mut self) -> &mut GraphBackend {
        match self.backend.as_mut().expect("tried to get redpiler backend when inactive") {
            ActiveBackend::Graph(graph) => graph,
        }
    }

    /// Run one logical tick without giving command blocks access to the world.
    ///
    /// Returns an error if a command host is needed; use [`Self::tick_with_world`]
    /// for those circuits. Call [`Self::flush`] to display changes.
    pub fn tick(&mut self) -> Result<(), GraphExecutionError> {
        match self.backend.as_mut().expect("compiler inactive") {
            ActiveBackend::Graph(graph) => graph.tick(),
        }
    }

    /// Use a lever or button and send its notifications immediately.
    ///
    /// Ignores other positions. Logs execution errors.
    pub fn on_use_block(&mut self, pos: BlockPos) {
        if let Err(error) = self.try_on_use_block(pos) {
            tracing::error!(%error, "graph backend control failed");
        }
    }

    /// Use a control and return any execution error.
    pub fn try_on_use_block(&mut self, pos: BlockPos) -> Result<(), GraphExecutionError> {
        match self.backend.as_mut().expect("compiler inactive") {
            ActiveBackend::Graph(graph) => graph.use_block(pos),
        }
    }

    pub(crate) fn accepts_control(&self, pos: BlockPos, plate: bool) -> bool {
        self.backend.as_ref().is_some_and(|backend| match backend {
            ActiveBackend::Graph(graph) => graph.accepts_control(pos, plate),
        })
    }

    /// Change a pressure plate's powered state and send its notifications immediately.
    ///
    /// Ignores other positions. Logs execution errors.
    pub fn set_pressure_plate(&mut self, pos: BlockPos, powered: bool) {
        if let Err(error) = self.try_set_pressure_plate(pos, powered) {
            tracing::error!(%error, "graph backend control failed");
        }
    }

    /// Set a plate and return any graph execution error to a headless host.
    pub fn try_set_pressure_plate(&mut self, pos: BlockPos, powered: bool) -> Result<(), GraphExecutionError> {
        match self.backend.as_mut().expect("compiler inactive") {
            ActiveBackend::Graph(graph) => graph.set_pressure_plate(pos, powered),
        }
    }

    /// Write display changes and effects to the world, keeping the circuit running.
    ///
    /// Applies [`CompilerOptions::io_only`]. Pending work stays in the backend.
    pub fn flush<W: World>(&mut self, world: &mut W) {
        if let Err(error) = self.try_flush(world) {
            tracing::error!(%error, "compiled projection failed");
        }
    }

    /// Project the compiled graph state.
    pub fn try_flush<W: World>(&mut self, world: &mut W) -> Result<(), GraphExecutionError> {
        let io_only = self.options.io_only;
        match self.backend.as_mut().expect("compiler inactive") {
            ActiveBackend::Graph(graph) => {
                graph.flush(world, io_only);
                Ok(())
            }
        }
    }

    /// Run one logical tick with world access for command blocks.
    ///
    /// Commands can read and change the world. Call [`Self::flush`] to display
    /// circuit changes. Returns any execution error.
    pub fn tick_with_world<W: World>(&mut self, world: &mut W) -> Result<(), GraphExecutionError> {
        match self.backend.as_mut().expect("compiler inactive") {
            ActiveBackend::Graph(graph) => graph.tick_with_world(world),
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
            budget_multiplier: 0,
            io_only: true,
            optimization_level: Some(0),
            update: true,
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
                assert_eq!(options.optimization_label(), format!("O{level}"));
            }
            assert_eq!(CompilerOptions::parse(prefix).unwrap().optimization_level, None);
            assert_eq!(CompilerOptions::parse(prefix).unwrap().optimization_label(), "O6");
            assert!(CompilerOptions::parse(&format!("{prefix}{}", MAX_OPTIMIZATION_LEVEL + 1)).is_err());
        }
        assert_eq!(CompilerOptions::parse("--optimize").unwrap().optimization_label(), "O6");
        assert!(CompilerOptions::parse("--stable-instant-inputs").is_err());
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
        for flags in ["--unknown", "--export", "--export-dot", "--classic-redstone", "-ox", "-"] {
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
