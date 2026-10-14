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

/// Work performed by the graph backend since activation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Cumulative work counters for an active graph backend.
pub struct GraphExecutionCounts {
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

#[cfg(feature = "graph-profile")]
impl GraphExecutionProfile {
    /// Returns the inclusive counters accumulated after a snapshot.
    pub fn delta_from(self, before: Self) -> Self {
        let delta = |after: GraphProfileMetric, prior: GraphProfileMetric| GraphProfileMetric {
            calls: after.calls.saturating_sub(prior.calls),
            elapsed_ns: after.elapsed_ns.saturating_sub(prior.elapsed_ns),
        };
        Self {
            tick_phases: delta(self.tick_phases, before.tick_phases),
            scheduled_drain: delta(self.scheduled_drain, before.scheduled_drain),
            piston_actions: delta(self.piston_actions, before.piston_actions),
            deferred_connections: delta(self.deferred_connections, before.deferred_connections),
            route_delivery: delta(self.route_delivery, before.route_delivery),
            turbo_wire_walk: delta(self.turbo_wire_walk, before.turbo_wire_walk),
            piston_sample: delta(self.piston_sample, before.piston_sample),
        }
    }
}

/// Callback work counters available only in an opt-in `graph-profile` build.
#[cfg(feature = "graph-profile")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct GraphCallbackProfile {
    /// Number of general ordered routes entered.
    pub route_calls: u64,
    /// Indexed by `ROUTE_ROLES`; roles are recovered from route owners.
    pub route_calls_by_role: [u64; 12],
    /// Number of ordered delivery records examined by general route calls.
    pub delivery_records_examined: u64,
    pub delivery_records_by_role: [u64; 12],
    /// Number of top-level guard predicates evaluated by routes or Turbo lookup.
    pub guard_roots_evaluated: u64,
    /// Counts actual recursive operations: Always, State, Site, Not, All, Any.
    pub guard_operations_by_kind: [u64; 6],
    /// Number of top-level guards that evaluated to false.
    pub guard_failures: u64,
    /// Number of captured site operands missing during guard evaluation.
    pub missing_captured_guard_operands: u64,
    /// Number of captured site operands missing before delivery dispatch.
    pub missing_captured_delivery_operands: u64,
    /// Number of passing delivery rows with no runtime fallback or receiver.
    pub deliveries_without_effective_receiver: u64,
    /// Rows are Component, PistonBase, PistonHead, Contact; columns are Update, Shape.
    /// Number of callbacks actually dispatched by compiled receiver and event kind.
    pub dispatches_by_receiver_and_kind: [[u64; 2]; 4],
    /// Rows use `ROUTE_ROLES`; columns are Update, Shape.
    /// Number of callbacks actually dispatched by owner role and event kind.
    pub dispatches_by_role_and_kind: [[u64; 2]; 12],
    /// Families are observer, wire, repeater, comparator, torch, lamp, bulb, command, note, control, piston, other.
    /// Number of concrete component or piston handlers invoked by family.
    pub handler_invocations_by_family: [u64; 12],
    /// Counts changed writes through component, wire, command, and note state handlers.
    /// Number of changed component, wire, command, and note state writes.
    pub component_state_changes: u64,
    /// Number of attempts to schedule component work, including rejected/duplicate work.
    pub tick_schedule_attempts: u64,
    /// Number of scheduler insertions that added previously absent work.
    pub newly_scheduled_ticks: u64,
    /// Number of piston samples returned before the normal sample counter due to a pending base.
    pub piston_samples_skipped_base_pending: u64,
    /// Number of ordered Turbo route candidate searches.
    pub turbo_route_lookups: u64,
    /// Number of Turbo searches served by a per-wire target index.
    pub turbo_indexed_lookups: u64,
    /// Number of Turbo searches that used the general linear route scan.
    pub turbo_fallback_lookups: u64,
    /// Number of candidate delivery rows inspected across Turbo searches.
    pub turbo_candidate_entries_scanned: u64,
    /// Inspected Turbo rows whose target matched the requested site.
    pub turbo_target_entries_scanned: u64,
    /// Number of Turbo searches that found no matching captured receiver.
    pub turbo_lookup_failures: u64,
    /// Number of Turbo searches that returned their first matching candidate.
    pub turbo_first_matches: u64,
    /// Number of nested wire nodes whose captured power was recomputed in Turbo.
    pub turbo_wire_node_evaluations: u64,
    /// Rows are Other, ScheduledTicks, PistonActions, DeferredConnections; columns use `ROUTE_ROLES`.
    pub route_calls_by_phase_and_role: [[u64; 12]; 4],
    /// Counts handler dispatches in the same phase/role layout.
    pub dispatches_by_phase_and_role: [[u64; 12]; 4],
}

#[cfg(feature = "graph-profile")]
impl GraphCallbackProfile {
    pub const ROUTE_ROLES: [&'static str; 12] = [
        "component_output",
        "wire_shape",
        "wire_turbo_candidates",
        "piston_base_changed",
        "piston_near_changed",
        "piston_far_changed",
        "piston_far_shape_changed",
        "piston_base_restored",
        "piston_head_restored",
        "piston_resource_restored_near",
        "piston_resource_restored_far",
        "unclassified",
    ];

    pub const PHASES: [&'static str; 4] = ["other", "scheduled_ticks", "piston_actions", "deferred_connections"];

    pub const GUARD_KINDS: [&'static str; 6] = ["always", "state", "site", "not", "all", "any"];

    pub const RECEIVER_FAMILIES: [&'static str; 4] = ["component", "piston_base", "piston_head", "contact"];

    pub const DELIVERY_KINDS: [&'static str; 2] = ["update", "shape"];

    pub const HANDLER_FAMILIES: [&'static str; 12] = [
        "observer",
        "wire",
        "repeater",
        "comparator",
        "torch",
        "lamp",
        "copper_bulb",
        "command_block",
        "note_block",
        "control",
        "piston",
        "other",
    ];

    pub fn delta_from(self, before: Self) -> Self {
        let delta = |after: u64, prior: u64| after.saturating_sub(prior);
        Self {
            route_calls: delta(self.route_calls, before.route_calls),
            route_calls_by_role: std::array::from_fn(|index| delta(self.route_calls_by_role[index], before.route_calls_by_role[index])),
            delivery_records_examined: delta(self.delivery_records_examined, before.delivery_records_examined),
            delivery_records_by_role: std::array::from_fn(|index| {
                delta(self.delivery_records_by_role[index], before.delivery_records_by_role[index])
            }),
            guard_roots_evaluated: delta(self.guard_roots_evaluated, before.guard_roots_evaluated),
            guard_operations_by_kind: std::array::from_fn(|index| {
                delta(self.guard_operations_by_kind[index], before.guard_operations_by_kind[index])
            }),
            guard_failures: delta(self.guard_failures, before.guard_failures),
            missing_captured_guard_operands: delta(self.missing_captured_guard_operands, before.missing_captured_guard_operands),
            missing_captured_delivery_operands: delta(self.missing_captured_delivery_operands, before.missing_captured_delivery_operands),
            deliveries_without_effective_receiver: delta(self.deliveries_without_effective_receiver, before.deliveries_without_effective_receiver),
            dispatches_by_receiver_and_kind: std::array::from_fn(|family| {
                std::array::from_fn(|kind| {
                    delta(
                        self.dispatches_by_receiver_and_kind[family][kind],
                        before.dispatches_by_receiver_and_kind[family][kind],
                    )
                })
            }),
            dispatches_by_role_and_kind: std::array::from_fn(|role| {
                std::array::from_fn(|kind| {
                    delta(
                        self.dispatches_by_role_and_kind[role][kind],
                        before.dispatches_by_role_and_kind[role][kind],
                    )
                })
            }),
            handler_invocations_by_family: std::array::from_fn(|index| {
                delta(self.handler_invocations_by_family[index], before.handler_invocations_by_family[index])
            }),
            component_state_changes: delta(self.component_state_changes, before.component_state_changes),
            tick_schedule_attempts: delta(self.tick_schedule_attempts, before.tick_schedule_attempts),
            newly_scheduled_ticks: delta(self.newly_scheduled_ticks, before.newly_scheduled_ticks),
            piston_samples_skipped_base_pending: delta(self.piston_samples_skipped_base_pending, before.piston_samples_skipped_base_pending),
            turbo_route_lookups: delta(self.turbo_route_lookups, before.turbo_route_lookups),
            turbo_indexed_lookups: delta(self.turbo_indexed_lookups, before.turbo_indexed_lookups),
            turbo_fallback_lookups: delta(self.turbo_fallback_lookups, before.turbo_fallback_lookups),
            turbo_candidate_entries_scanned: delta(self.turbo_candidate_entries_scanned, before.turbo_candidate_entries_scanned),
            turbo_target_entries_scanned: delta(self.turbo_target_entries_scanned, before.turbo_target_entries_scanned),
            turbo_lookup_failures: delta(self.turbo_lookup_failures, before.turbo_lookup_failures),
            turbo_first_matches: delta(self.turbo_first_matches, before.turbo_first_matches),
            turbo_wire_node_evaluations: delta(self.turbo_wire_node_evaluations, before.turbo_wire_node_evaluations),
            route_calls_by_phase_and_role: std::array::from_fn(|phase| {
                std::array::from_fn(|role| {
                    delta(
                        self.route_calls_by_phase_and_role[phase][role],
                        before.route_calls_by_phase_and_role[phase][role],
                    )
                })
            }),
            dispatches_by_phase_and_role: std::array::from_fn(|phase| {
                std::array::from_fn(|role| {
                    delta(
                        self.dispatches_by_phase_and_role[phase][role],
                        before.dispatches_by_phase_and_role[phase][role],
                    )
                })
            }),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
/// Options that control graph preparation and reset behavior.
pub struct CompilerOptions {
    /// Set by the server from the initiating player's rank, never from flags.
    /// Zero retains the default 1x budget; values are capped at 8x.
    pub budget_multiplier: usize,
    /// `Some(level)` selects that pass tier; `None` selects every configured tier.
    pub optimization_level: Option<usize>,
    /// Only flush visible outputs and controls, including lamps and copper bulbs.
    pub io_only: bool,
    /// Update all blocks in the input region after reset.
    pub update: bool,
}

pub(crate) const MAX_OPTIMIZATION_LEVEL: usize = 2;

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            budget_multiplier: 0,
            optimization_level: Some(0),
            io_only: false,
            update: false,
        }
    }
}

impl CompilerOptions {
    pub fn parse(flags: &str) -> Result<CompilerOptions, String> {
        let mut options = Self::default();
        for option in flags.split_whitespace() {
            if let Some(level) = option.strip_prefix("-O").or_else(|| option.strip_prefix("-o")) {
                options.optimization_level = if level.is_empty() {
                    None
                } else {
                    let level = level
                        .parse::<usize>()
                        .map_err(|_| format!("Invalid Redpiler optimization level: {option}"))?;
                    if level > MAX_OPTIMIZATION_LEVEL {
                        return Err(format!("Redpiler supports optimization levels 0 through {MAX_OPTIMIZATION_LEVEL}"));
                    }
                    Some(level)
                };
                continue;
            }
            if option.starts_with("--") {
                match option {
                    "--optimize" => options.optimization_level = None,
                    "--io-only" => options.io_only = true,
                    "--update" => options.update = true,
                    _ => return Err(format!("Unrecognized Redpiler option: {option}")),
                }
            } else if let Some(short_flags) = option.strip_prefix('-') {
                if short_flags.is_empty() {
                    return Err(format!("Unrecognized Redpiler option: {option}"));
                }
                for (index, c) in short_flags.char_indices() {
                    match c.to_ascii_lowercase() {
                        'o' => {
                            let level = &short_flags[index + c.len_utf8()..];
                            options.optimization_level = if level.is_empty() {
                                None
                            } else {
                                let level = level
                                    .parse::<usize>()
                                    .map_err(|_| format!("Invalid Redpiler optimization level: {option}"))?;
                                if level > MAX_OPTIMIZATION_LEVEL {
                                    return Err(format!("Redpiler supports optimization levels 0 through {MAX_OPTIMIZATION_LEVEL}"));
                                }
                                Some(level)
                            };
                            break;
                        }
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
/// Node and link counts for one compiler graph snapshot.
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
/// Timing and size results for one graph pass.
pub struct PassStatistics {
    pub name: &'static str,
    pub enabled: bool,
    pub before: GraphCounts,
    pub after: GraphCounts,
    pub duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Retained-capacity estimate for one preparation table.
pub struct PreparationTableStatistics {
    pub stage: &'static str,
    pub table: String,
    pub entries: usize,
    pub capacity: usize,
    /// Estimates retained element storage and known nested slices, excluding maps and allocator overhead.
    pub estimated_bytes: usize,
}

#[derive(Clone, Debug, Default)]
/// Graph sizes, pass timings, and preparation memory estimates from compilation.
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
                before.nodes,
                before.links,
                after.nodes,
                after.links,
                after.nodes as i128 - before.nodes as i128,
                after.links as i128 - before.links as i128,
            ));
            lines.push("Construction: all parsed wire nodes are retained.".into());
        }
        if let Some(stage) = self.preparation_tables.last().map(|table| table.stage) {
            let mut tables: Vec<_> = self.preparation_tables.iter().filter(|table| table.stage == stage).collect();
            tables.sort_by_key(|table| std::cmp::Reverse(table.estimated_bytes));
            let estimate = tables.iter().map(|table| table.estimated_bytes).sum::<usize>();
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
        let enabled = self.transformations.iter().filter(|pass| pass.enabled).count();
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
/// Timing and backend size results for one successful compilation.
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
        lines.extend(self.graph.summary_lines().into_iter().take(3));
        lines
    }
}

#[derive(Default)]
/// Owns the active compiled backend and its options and statistics.
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

    /// Callback work counters for profiling-window snapshots.
    #[cfg(feature = "graph-profile")]
    pub fn graph_callback_profile(&self) -> Option<GraphCallbackProfile> {
        self.backend.as_ref()?.graph_callback_profile()
    }

    /// Static route and guard inventory for a compiled graph, collected on demand.
    #[cfg(feature = "graph-profile")]
    pub fn graph_callback_inventory(&self) -> Option<serde_json::Value> {
        self.backend.as_ref()?.graph_callback_inventory()
    }

    /// Actual canonical neighbor-power samples for diagnostic long dust candidates.
    #[cfg(feature = "graph-profile")]
    pub fn dust_candidate_power_samples(&self) -> Option<Vec<(usize, u64)>> {
        Some(self.backend.as_ref()?.dust_candidate_power_samples())
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
        self.publish(backend, options, &monitor, start, analysis_duration, backend_start)
    }

    fn prepare_graph_backend<W: World>(
        world: &W,
        bounds: (BlockPos, BlockPos),
        ticks: Vec<TickEntry>,
        options: &CompilerOptions,
        monitor: &TaskMonitor,
        graph_pipeline: Option<passes::PassPipeline>,
    ) -> Result<(Runtime, Duration), CompileError> {
        let limits = analysis::AnalysisLimits::for_budget(monitor.budget_multiplier());
        let mut prepared = passes::rich::prepare(world, bounds, ticks, monitor, limits).map_err(|error| graph_preparation_error(error, monitor))?;
        let analysis_duration = prepared.analysis_duration;
        let pipeline = graph_pipeline.unwrap_or_else(|| passes::default_graph_pipeline(options));
        passes::run_graph_pipeline(&mut prepared.graph, options, &prepared.entry, &pipeline, monitor)
            .map_err(|error| graph_pipeline_error(error, monitor))?;
        prepared.finish_pipeline_statistics(monitor);
        let lowered = prepared
            .lower(&monitor)
            .map_err(|error| graph_backend_error(backend::graph::GraphBackendError::Graph(error), monitor))?;
        let backend = backend::graph::GraphBackend::from_lowered(lowered, monitor).map_err(|error| graph_backend_error(error, monitor))?;
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
        self.backend.as_mut().expect("tried to get redpiler backend when inactive")
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
