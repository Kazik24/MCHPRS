//! Compile world blocks into an electrical graph, then execute it with the direct
//! backend. Piston regions execute logical plans and export settled geometry.

pub mod analysis;
pub(crate) mod backend;
mod compile_graph;
pub mod instant;
mod passes;
mod task_monitor;

use crate::redstone;
use crate::world::{for_each_block_mut_optimized, World};
use backend::{direct::DirectBackend, Runtime};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;
use mchprs_world::TickEntry;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, trace};

pub use backend::{Queues, TickScheduler};
pub use task_monitor::TaskMonitor;

#[derive(Debug)]
pub enum CompileError {
    AlreadyActive,
    Analysis(analysis::AnalysisError),
    Unsupported(Box<analysis::AnalysisReport>),
    Cancelled,
    Backend(backend::BackendError),
    Graph(compile_graph::GraphError),
    Instant(String),
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
            Self::Backend(error) => error.fmt(f),
            Self::Graph(error) => error.fmt(f),
            Self::Instant(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for CompileError {}

fn block_powered_mut(block: &mut Block) -> Option<&mut bool> {
    Some(match block {
        Block::RedstoneComparator { comparator } => &mut comparator.powered,
        Block::RedstoneTorch { lit } => lit,
        Block::RedstoneWallTorch { lit, .. } => lit,
        Block::RedstoneRepeater { repeater } => &mut repeater.powered,
        Block::Observer { observer } => &mut observer.powered,
        Block::Lever { lever } => &mut lever.powered,
        Block::StoneButton { button } => &mut button.powered,
        Block::StonePressurePlate { powered } => powered,
        Block::RedstoneLamp { lit } => lit,
        Block::IronTrapdoor { powered, .. } => powered,
        Block::NoteBlock { powered, .. } => powered,
        _ => return None,
    })
}

#[derive(Default, PartialEq, Eq, Debug)]
pub struct CompilerOptions {
    /// Trust instant/BUD construction, while validating logical data and sampling roles.
    pub assume_instant: bool,
    /// Set by the server from the initiating player's rank, never from flags.
    /// Zero retains the default 1x budget; values are capped at 8x.
    pub budget_multiplier: usize,
    /// Enable optimization passes which may significantly increase compile times.
    pub optimize: bool,
    /// Export the graph to a binary format. See the [`redpiler_graph`] crate.
    pub export: bool,
    /// Only flush visible outputs and controls, including lamps and copper bulbs.
    pub io_only: bool,
    /// Update all blocks in the input region after reset.
    pub update: bool,
    /// Export a dot file of the backend graph after compilation.
    pub export_dot_graph: bool,
}

impl CompilerOptions {
    pub fn parse(flags: &str) -> Result<CompilerOptions, String> {
        let mut options = Self::default();
        for option in flags.split_whitespace() {
            if option.starts_with("--") {
                match option {
                    "--assume-instant" => options.assume_instant = true,
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
    pub(crate) fn of(graph: &compile_graph::CompileGraph) -> Self {
        Self {
            nodes: graph.node_count(),
            links: graph.edge_count(),
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

#[derive(Clone, Debug, Default)]
pub struct GraphStatistics {
    /// Native dust/component execution retains physical identities and skips rewrites.
    pub native_propagation: bool,
    /// Ordinary wire nodes are omitted by identify_nodes when -O is enabled.
    /// This occurs before the optimization baseline, not in an optional pass.
    pub wire_nodes_elided: bool,
    pub passes: Vec<PassStatistics>,
    pub duration: Duration,
}

impl GraphStatistics {
    pub fn baseline(&self) -> Option<GraphCounts> {
        self.passes.get(2).map(|pass| pass.after)
    }

    pub fn final_graph(&self) -> Option<GraphCounts> {
        self.passes.last().map(|pass| pass.after)
    }

    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.native_propagation {
            lines.push("Execution: native redstone propagation over a private snapshot; graph optimization skipped.".into());
        }
        if let (Some(before), Some(after)) = (self.baseline(), self.final_graph()) {
            lines.push(format!(
                "Graph after required preparation: {} nodes / {} links; after passes: {} nodes / {} links (change {:+} nodes / {:+} links)",
                before.nodes, before.links, after.nodes, after.links,
                after.nodes as i128 - before.nodes as i128,
                after.links as i128 - before.links as i128,
            ));
            lines.push(if self.wire_nodes_elided {
                "Construction: ordinary wire nodes elided before the baseline (-O).".into()
            } else {
                "Construction: ordinary wire-node elision disabled; optional optimization passes disabled.".into()
            });
        }
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

#[derive(Clone, Debug, Default)]
pub struct RegionStatistics {
    pub logical_regions: usize,
    pub clocked_regions: usize,
    pub pistons: usize,
    pub payload_groups: usize,
    pub memory_cells: usize,
    /// Complete bound arena, including restoration-only decisions.
    pub decisions: usize,
    pub response_roots: usize,
    pub output_ports: usize,
    pub output_terms: usize,
    pub logical_response_decisions: usize,
    /// Output guards and independent sampling guards, excluding response work.
    pub logical_output_decisions: usize,
    /// Unique bindings per logical plan; plans may overlap.
    pub logical_input_bindings: usize,
}

#[derive(Clone, Debug)]
pub struct CompileStatistics {
    pub graph: GraphStatistics,
    pub regions: RegionStatistics,
    pub backend_nodes: usize,
    pub analysis_duration: Duration,
    pub preparation_duration: Duration,
    pub backend_duration: Duration,
    pub total_duration: Duration,
}

impl CompileStatistics {
    pub fn summary_lines(&self) -> Vec<String> {
        let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
        let mut lines = vec![format!(
            "Compile: {:.3} ms total; analysis {:.3}, instant preparation {:.3}, graph {:.3}, backend {:.3} ms; {} backend nodes",
            ms(self.total_duration), ms(self.analysis_duration), ms(self.preparation_duration),
            ms(self.graph.duration), ms(self.backend_duration), self.backend_nodes,
        )];
        lines.extend(self.graph.summary_lines().into_iter().take(2));
        let regions = &self.regions;
        if regions.pistons != 0 {
            lines.push(format!(
                "Piston executors: {} logical regions; {} clocked; {} pistons / {} payload groups / {} sampled memory cells",
                regions.logical_regions, regions.clocked_regions, regions.pistons,
                regions.payload_groups, regions.memory_cells,
            ));
            lines.push(format!(
                "Programs: {} arena decisions (including handoff), {} response roots, {} output ports / {} terms",
                regions.decisions, regions.response_roots, regions.output_ports, regions.output_terms,
            ));
            if regions.logical_regions != 0 {
                lines.push(format!(
                    "Logical plans: {} response decisions / {} output and sampling decisions / {} input bindings",
                    regions.logical_response_decisions, regions.logical_output_decisions,
                    regions.logical_input_bindings,
                ));
            }
        }
        lines
    }
}

#[derive(Default)]
pub struct Compiler {
    backend: Option<Runtime>,
    options: CompilerOptions,
    warnings: Vec<String>,
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

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
    pub fn is_active(&self) -> bool {
        self.backend.is_some()
    }

    pub fn current_flags(&self) -> Option<&CompilerOptions> {
        self.backend.as_ref().map(|_| &self.options)
    }

    pub fn compile<W: World>(
        &mut self,
        world: &W,
        bounds: (BlockPos, BlockPos),
        options: CompilerOptions,
        ticks: Vec<TickEntry>,
        monitor: Arc<TaskMonitor>,
    ) -> Result<(), CompileError> {
        let span = tracing::info_span!("redpiler", ?bounds, ?options);
        let _entered = span.enter();
        tracing::info!("Compiler started");
        debug!("Starting compile");
        let start = Instant::now();

        if self.is_active() {
            return Err(CompileError::AlreadyActive);
        }
        self.warnings.clear();
        monitor.clear_graph_statistics();
        monitor.set_budget_multiplier(options.budget_multiplier);
        let analysis_start = Instant::now();
        let report = analysis::analyze(
            world,
            bounds,
            &ticks,
            &monitor,
            analysis::AnalysisLimits::for_budget(monitor.budget_multiplier()),
        )
        .map_err(CompileError::Analysis)?;
        let analysis_duration = analysis_start.elapsed();
        if report.pistons.is_empty() && !report.can_compile() {
            return Err(CompileError::Unsupported(Box::new(report)));
        }

        let ticks: Vec<_> = ticks
            .into_iter()
            .filter(|entry| {
                entry
                    .block_type
                    .is_none_or(|kind| world.get_block(entry.pos).registry_id() == kind)
            })
            .collect();
        let input = CompilerInput {
            world,
            bounds,
            ticks: &ticks,
            boundaries: None,
        };
        let preparation_start = Instant::now();
        let (graph, instant, native) = if report.pistons.is_empty() {
            let (graph, native) =
                passes::prepare(&options, &input, &monitor, true).map_err(CompileError::Graph)?;
            (graph, Vec::new(), native)
        } else {
            let (graph, program) =
                instant::program::prepare(world, &report, &ticks, &options, monitor.clone())
                    .map_err(CompileError::Instant)?;
            (graph, program, false)
        };
        let preparation_and_graph_duration = preparation_start.elapsed();
        let graph_statistics = monitor.graph_statistics();
        let preparation_duration = if report.pistons.is_empty() {
            Duration::ZERO
        } else {
            preparation_and_graph_duration.saturating_sub(graph_statistics.duration)
        };

        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }

        // Stage a fresh backend. Reusing one can leave aliases, scheduler work
        // or side tables from a previous compilation. Publish only on success.
        trace!("Compiling backend");
        monitor.set_message("Compiling backend".to_string());
        let backend_start = Instant::now();
        let backend = if native {
            if options.export_dot_graph {
                return Err(CompileError::Graph(
                    compile_graph::GraphError::UnsupportedNativeExport,
                ));
            }
            Runtime::native(world, bounds, &graph, ticks, &monitor)?
        } else {
            let mut backend = DirectBackend::default();
            backend
                .compile(graph, ticks, &options, instant)
                .map_err(CompileError::Backend)?;
            Runtime::Direct(backend)
        };
        let backend_duration = backend_start.elapsed();
        if monitor.cancelled() {
            return Err(CompileError::Cancelled);
        }
        monitor.inc_progress();

        let statistics = CompileStatistics {
            graph: graph_statistics,
            regions: backend.region_statistics(),
            backend_nodes: backend.node_count(),
            analysis_duration,
            preparation_duration,
            backend_duration,
            total_duration: start.elapsed(),
        };
        self.backend = Some(backend);
        self.statistics = Some(statistics);
        self.options = options;
        self.warnings = report.pistons.iter()
            .filter(|p| p.piston.extended && p.diagnostics.contains(&analysis::PistonDiagnostic::MissingOrMismatchedHead))
            .map(|p| format!("Extended piston at {:?} has no matching saved head at {:?}; the runtime starts from its saved geometry.", p.pos, p.head))
            .collect();
        tracing::info!(stats = ?self.statistics, "Compiler completed");
        for warning in &self.warnings {
            tracing::warn!("Redpiler: {warning}");
        }
        debug!("Compile completed in {:?}", start.elapsed());
        Ok(())
    }

    pub fn reset<W: World>(&mut self, world: &mut W, bounds: (BlockPos, BlockPos)) {
        self.warnings.clear();
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

    pub fn tick(&mut self) {
        self.backend().tick();
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

    pub fn tick_with_world<W: World>(&mut self, world: &mut W) {
        self.backend().tick_with_world(world);
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
    pub(crate) boundaries: Option<&'w instant::boundary::Boundaries<'w>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_options() {
        let input = "-iO -U --export";
        let expected_options = CompilerOptions {
            assume_instant: false,
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
    fn logical_mode_is_explicit_and_unknown_flags_fail() {
        assert!(
            CompilerOptions::parse("--assume-instant -oi")
                .unwrap()
                .assume_instant
        );
        assert!(!CompilerOptions::parse("").unwrap().assume_instant);
        for flags in ["--assume-instnat", "-ox", "-", "assume-instant"] {
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
        assert_eq!(statistics.graph.passes.len(), 10);
        assert_eq!(statistics.graph.baseline(), statistics.graph.final_graph());
        assert_eq!(statistics.regions.pistons, 0);
        assert!(statistics.backend_nodes > 0);
        let total_duration = statistics.total_duration;
        compiler.reset(&mut world, bounds);
        let cancelled = Arc::new(TaskMonitor::default());
        cancelled.cancel();
        assert!(compiler
            .compile(
                &world,
                bounds,
                CompilerOptions::default(),
                Vec::new(),
                cancelled
            )
            .is_err());
        assert_eq!(compiler.stats().unwrap().total_duration, total_duration);
    }
}
