//! Graph rewrites and execution-plan builders.
//!
//! Rewrites change the graph and discard old plans. Builders read the graph and
//! add execution plans for supported cases. The runner checks the input and each
//! enabled pass's result before lowering. See the [redpiler overview](crate::redpiler).
pub(crate) mod coalesce_delayed_updates;
pub(crate) mod compile_bud_memory_reads;
pub(crate) mod compile_builtin_resets;
pub(crate) mod compile_classic_redstone;
mod compile_cpu_power_queries;
pub(crate) mod compile_fixed_classic;
mod compile_fixed_signals;
mod compile_guarded_instant_transport;
mod compile_instant_drives;
pub(crate) mod compile_instant_events;
pub(crate) mod compile_instant_layers;
pub(crate) mod compile_instant_proxies;
pub(crate) mod compile_instant_resets;
mod compile_instant_transition_programs;
pub(crate) mod compile_instant_transport;
mod compile_instant_value_regions;
pub(crate) mod compile_instant_wire_nodes;
pub(crate) mod compile_logical_bud_owners;
pub(crate) mod compile_mixed_classic;
mod compile_settled_circuit;
mod compile_signal_channels;
mod compile_stable_instant_logic;
pub(crate) mod compile_wire_callbacks;
pub(crate) mod compile_wire_captures;
pub(crate) mod compile_wire_hops;
pub(crate) mod compile_wire_shapes;
pub(crate) mod contract_dust_paths;
pub(crate) mod identify_instant_boundaries;
pub(crate) mod identify_instant_regions;
pub(crate) mod index_turbo_callbacks;
pub(crate) mod minimize_wire_captures;
pub(crate) mod optimize_instant_kernels_cpu;
pub(crate) mod planning;
pub(crate) mod prune_inert_callbacks;
pub(crate) mod prune_unhandled_updates;
pub(crate) mod rich;
pub(crate) mod simplify_guards;
pub(crate) mod simplify_instant_resets;
pub(crate) mod simplify_state_guards;
pub(crate) mod specialize_instant_pistons;
pub(crate) mod specialize_wire_queries;
pub(crate) mod utils;
mod validation;

use super::program_graph::{CompileGraph, EntryState, GraphBuildError};
use super::{CompilerOptions, GraphCounts, PassStatistics, TaskMonitor};
use std::fmt;
use std::time::Instant;

/// A pipeline step. Rewrites invalidate derived plans; read-only builders cannot change the circuit.
pub(crate) trait GraphPass {
    fn name(&self) -> &'static str;
    fn validate_input(&self, _graph: &CompileGraph) -> Result<(), String> {
        Ok(())
    }
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String>;
}

#[allow(dead_code)]
/// Shared compilation inputs available to each graph transformation.
pub(crate) struct PassContext<'a> {
    /// Canonical source analysis prepared by the runner before this builder executes.
    prepared_instant: Option<&'a validation::PreparedInstant>,
    /// This shares cancellation and progress reporting with compilation.
    pub(crate) monitor: &'a TaskMonitor,
    /// This exposes compiler settings without choosing which passes run.
    pub(crate) options: &'a CompilerOptions,
    /// This exposes entry time and pending work to transformation checks.
    pub(crate) entry: &'a EntryState,
}

impl<'a> PassContext<'a> {
    pub(crate) fn new(monitor: &'a TaskMonitor, options: &'a CompilerOptions, entry: &'a EntryState) -> Self {
        Self {
            monitor,
            options,
            entry,
            prepared_instant: None,
        }
    }

    pub(crate) fn instant_boundaries(&self) -> Option<&super::program_graph::BoundaryCatalog> {
        match self.prepared_instant {
            Some(validation::PreparedInstant::Boundaries(catalog)) => Some(catalog),
            _ => None,
        }
    }

    pub(crate) fn instant_layers(&self) -> Option<&super::program_graph::LayeredInstantPlan> {
        match self.prepared_instant {
            Some(validation::PreparedInstant::Layers { plan, .. }) => Some(plan),
            _ => None,
        }
    }
}

/// The variant determines access and invalidation; passes declare no preservation flags.
pub(crate) enum Pass {
    Rewrite(Box<dyn GraphPass>),
    Build(Box<dyn planning::PlanBuilder>),
}

impl Pass {
    pub(crate) fn rewrite(pass: impl GraphPass + 'static) -> Self {
        Self::Rewrite(Box::new(pass))
    }

    pub(crate) fn build(builder: impl planning::PlanBuilder + 'static) -> Self {
        Self::Build(Box::new(builder))
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Rewrite(pass) => pass.name(),
            Self::Build(builder) => builder.name(),
        }
    }

    fn validate_input(&self, graph: &CompileGraph) -> Result<(), String> {
        match self {
            Self::Rewrite(pass) => pass.validate_input(graph),
            Self::Build(_) => Ok(()),
        }
    }

    #[cfg(test)]
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        match self {
            Self::Rewrite(pass) => pass.run(graph, context),
            Self::Build(builder) => {
                let output = builder.build(graph, context)?;
                if context.monitor.cancelled() {
                    return Err("cancelled before publishing execution plans".into());
                }
                output.publish(graph).map_err(|error| error.to_string())?;
                Ok(())
            }
        }
    }
}

/// A graph transformation and whether this pipeline should run it.
pub(crate) struct PassEntry {
    /// This owns a built-in or developer-defined transformation.
    pub(crate) pass: Pass,
    /// This records whether the runner should validate and execute the pass.
    pub(crate) enabled: bool,
}

/// Ordered graph transformations used between preparation and lowering.
pub(crate) struct PassPipeline {
    /// This preserves the caller-supplied transformation order.
    pub(crate) entries: Vec<PassEntry>,
}

impl PassPipeline {
    #[allow(dead_code)]
    pub(crate) fn new(entries: Vec<PassEntry>) -> Self {
        Self { entries }
    }
}

impl Default for PassPipeline {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

#[derive(Debug)]
/// Failure while validating or running an ordered graph transformation pipeline.
pub(crate) enum PassPipelineError {
    /// Cancellation stopped the ordered pass sequence.
    Cancelled,
    /// A pass did not provide a usable stable name.
    EmptyName,
    /// Two entries claimed the same stable name.
    DuplicateName(&'static str),
    /// The parsed graph failed validation before any transformation ran.
    InvalidInput(GraphBuildError),
    /// A pass rejected its input before mutation.
    Precondition { pass: &'static str, message: String },
    /// A pass returned an execution error.
    Failed { pass: &'static str, message: String },
    /// A successful pass returned a graph that failed structural validation.
    MalformedOutput { pass: &'static str, source: GraphBuildError },
}

impl fmt::Display for PassPipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("graph pass pipeline cancelled"),
            Self::EmptyName => f.write_str("graph pass has an empty stable name"),
            Self::DuplicateName(name) => write!(f, "duplicate graph pass name {name:?}"),
            Self::InvalidInput(error) => write!(f, "invalid graph before passes: {error}"),
            Self::Precondition { pass, message } => {
                write!(f, "graph pass {pass:?} precondition failed: {message}")
            }
            Self::Failed { pass, message } => {
                write!(f, "graph pass {pass:?} failed: {message}")
            }
            Self::MalformedOutput { pass, source } => {
                write!(f, "graph pass {pass:?} produced an invalid graph: {source}")
            }
        }
    }
}

impl std::error::Error for PassPipelineError {}

// shortcut: this batch has no ID-changing passes; add remaps with the first such rewrite.
/// Run exactly the supplied entries; current transformations preserve typed IDs.
#[cfg(test)]
pub(crate) fn run_graph_pipeline(
    graph: &mut CompileGraph,
    options: &CompilerOptions,
    entry: &EntryState,
    pipeline: &PassPipeline,
    monitor: &TaskMonitor,
) -> Result<(), PassPipelineError> {
    run_graph_pipeline_with_analysis(graph, options, entry, pipeline, monitor).map(|_| ())
}

/// Run enabled passes in order and check each result.
/// Stops on cancellation or a failed check, before a backend is activated.
fn run_graph_pipeline_with_analysis(
    graph: &mut CompileGraph,
    options: &CompilerOptions,
    entry: &EntryState,
    pipeline: &PassPipeline,
    monitor: &TaskMonitor,
) -> Result<Option<crate::redpiler::analysis::instant::layers::LayerCompilationContext>, PassPipelineError> {
    if monitor.cancelled() {
        return Err(PassPipelineError::Cancelled);
    }
    let mut names = std::collections::HashSet::with_capacity(pipeline.entries.len());
    for item in &pipeline.entries {
        let name = item.pass.name();
        if name.trim().is_empty() {
            return Err(PassPipelineError::EmptyName);
        }
        if !names.insert(name) {
            return Err(PassPipelineError::DuplicateName(name));
        }
    }
    if monitor.cancelled() {
        return Err(PassPipelineError::Cancelled);
    }
    let mut layers = validation::input(graph, monitor)?;
    monitor.set_max_progress(monitor.progress() + pipeline.entries.len() + 1);
    let context = PassContext::new(monitor, options, entry);
    for item in &pipeline.entries {
        if monitor.cancelled() {
            return Err(PassPipelineError::Cancelled);
        }
        let name = item.pass.name();
        let before = GraphCounts::of(graph);

        if !item.enabled {
            monitor.record_transformation(PassStatistics::disabled(name, before));
            monitor.inc_progress();
            continue;
        }
        if matches!(item.pass, Pass::Rewrite(_)) {
            graph.clear_execution_plans();
        }
        monitor.set_message(name.to_string());
        let start = Instant::now();

        if let Err(message) = item.pass.validate_input(graph) {
            utils::record_transformation(monitor, name, before, GraphCounts::of(graph), start, std::time::Duration::ZERO);
            return Err(PassPipelineError::Precondition { pass: name, message });
        }
        let validation_duration = match validation::run_checked(&item.pass, graph, &context, &mut layers) {
            Ok(duration) => duration,
            Err(error) => {
                utils::record_transformation(monitor, name, before, GraphCounts::of(graph), start, std::time::Duration::ZERO);
                return Err(error);
            }
        };

        let after = GraphCounts::of(graph);

        #[cfg(feature = "graph-trace")]
        tracing::debug!(
            target: "mchprs_core::redpiler::passes",
            pass = name,
            before_nodes = before.nodes,
            after_nodes = after.nodes,
            nodes_removed = before.nodes.saturating_sub(after.nodes),
            before_links = before.links,
            after_links = after.links,
            elapsed_ns = start.elapsed().as_nanos(),
            "graph pass complete"
        );

        utils::record_transformation(monitor, name, before, after, start, validation_duration);

        monitor.inc_progress();

        if monitor.cancelled() {
            return Err(PassPipelineError::Cancelled);
        }
    }
    if monitor.cancelled() {
        return Err(PassPipelineError::Cancelled);
    }
    // Entry and every enabled output have already passed their required validation.
    monitor.set_final_graph(GraphCounts::of(graph));
    Ok(layers)
}

/// Choose passes from the compiler options, with rewrites before plan builders.
/// Each builder decides whether the circuit fits its optimization.
pub(crate) fn default_graph_pipeline(options: &CompilerOptions) -> PassPipeline {
    let level = options.optimization_level.unwrap_or(usize::MAX);
    let rewrites: [(usize, Pass); 8] = [
        (1, Pass::rewrite(simplify_guards::GuardAndUpdateReduction)),
        (1, Pass::rewrite(prune_inert_callbacks::PruneInertCallbacks)),
        (1, Pass::rewrite(prune_unhandled_updates::PruneUnhandledUpdates)),
        (2, Pass::rewrite(simplify_state_guards::SimplifyStateGuards)),
        (3, Pass::rewrite(simplify_instant_resets::SimplifyInstantResets)),
        (3, Pass::rewrite(specialize_instant_pistons::SpecializeInstantPistons)),
        (3, Pass::rewrite(compile_builtin_resets::CompileBuiltinResets)),
        (5, Pass::rewrite(coalesce_delayed_updates::CoalesceDelayedUpdates)),
    ];
    let plans: [(usize, Pass); 11] = [
        (1, Pass::build(index_turbo_callbacks::IndexTurboCallbacks)),
        (1, Pass::build(contract_dust_paths::ContractDustPaths)),
        (2, Pass::build(specialize_wire_queries::SpecializeWireQueries)),
        (3, Pass::build(compile_wire_shapes::CompileWireShapes)),
        (3, Pass::build(compile_wire_captures::CompileWireCaptures)),
        (2, Pass::build(compile_wire_hops::CompileWireHops)),
        (2, Pass::build(compile_wire_callbacks::CompileWireCallbacks)),
        (3, Pass::build(compile_instant_resets::CompileInstantResets)),
        (3, Pass::build(identify_instant_regions::IdentifyInstantRegions)),
        (3, Pass::build(compile_instant_proxies::CompileInstantProxies)),
        (2, Pass::build(compile_fixed_signals::CompileFixedSignals)),
    ];
    let entries = rewrites
        .into_iter()
        .map(|(minimum_level, pass)| PassEntry {
            pass,
            enabled: level >= minimum_level,
        })
        .chain(plans.into_iter().map(|(minimum_level, pass)| PassEntry {
            pass,
            enabled: level >= minimum_level,
        }))
        .chain(std::iter::once(PassEntry {
            pass: Pass::build(compile_fixed_classic::CompileFixedClassic),
            enabled: level >= 2,
        }))
        .chain(std::iter::once(PassEntry {
            pass: Pass::build(compile_mixed_classic::CompileMixedClassic),
            enabled: level >= 2,
        }))
        .chain(std::iter::once(PassEntry {
            pass: Pass::build(compile_classic_redstone::CompileClassicRedstone),
            enabled: level >= 2,
        }))
        .collect();
    let mut pipeline = PassPipeline::new(entries);
    if level >= 4 {
        pipeline
            .entries
            .retain(|entry| !matches!(entry.pass.name(), "identify-instant-regions" | "compile-instant-proxies"));
    }
    // FIFO-local entries sample live inputs; stable logic checks its explicit promise separately.
    let kernels = [
        (4, Pass::build(identify_instant_boundaries::IdentifyInstantBoundaries)),
        (4, Pass::build(compile_instant_wire_nodes::CompileInstantWireNodes)),
        (4, Pass::build(minimize_wire_captures::MinimizeWireCaptures)),
        (4, Pass::build(compile_bud_memory_reads::CompileBudMemoryReads)),
        (4, Pass::build(compile_instant_events::CompileInstantEvents)),
        (6, Pass::build(compile_logical_bud_owners::CompileLogicalBudOwners)),
        (5, Pass::build(optimize_instant_kernels_cpu::OptimizeInstantKernelsCpu)),
    ];
    pipeline.entries.extend(kernels.into_iter().map(|(minimum_level, pass)| PassEntry {
        pass,
        enabled: level >= minimum_level,
    }));
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_instant_transport::CompileInstantTransport),
        enabled: level >= 5,
    });
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_guarded_instant_transport::CompileGuardedInstantTransport),
        enabled: level >= 5,
    });
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_instant_drives::CompileInstantDrives),
        enabled: level >= 5,
    });
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_instant_transition_programs::CompileInstantTransitionPrograms),
        enabled: level >= 5,
    });
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_instant_value_regions::CompileInstantValueRegions),
        enabled: level >= 5,
    });
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_signal_channels::CompileSignalChannels),
        enabled: level >= 5,
    });
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_stable_instant_logic::CompileStableInstantLogic),
        enabled: level >= 5,
    });
    if level >= 6 && options.stable_instant_inputs {
        pipeline.entries.push(PassEntry {
            pass: Pass::build(compile_instant_layers::CompileInstantLayers),
            enabled: true,
        });
    }
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_settled_circuit::CompileSettledCircuit),
        enabled: level >= 6,
    });
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_cpu_power_queries::CompileCpuPowerQueries),
        enabled: level >= 6,
    });
    pipeline
}

/// Inspection-only replacement for the old region/proxy builders; public tiers stay unchanged.
#[cfg(test)]
pub(crate) fn experimental_boundary_pipeline(options: &CompilerOptions) -> PassPipeline {
    let mut pipeline = default_graph_pipeline(options);
    pipeline.entries.retain(|entry| {
        !matches!(
            entry.pass.name(),
            "identify-instant-regions"
                | "compile-instant-proxies"
                | "identify-instant-boundaries"
                | "compile-instant-layers"
                | "experimental-compile-instant-wire-nodes"
                | "experimental-minimize-wire-captures"
                | "experimental-compile-bud-memory-reads"
                | "experimental-compile-instant-events"
                | "optimize-instant-kernels-cpu"
        )
    });
    pipeline.entries.push(PassEntry {
        pass: Pass::build(identify_instant_boundaries::IdentifyInstantBoundaries),
        enabled: options.optimization_level.unwrap_or(usize::MAX) >= 3,
    });
    pipeline
}

/// Build cold causal layers without enabling a kernel or changing public optimization tiers.
#[cfg(test)]
pub(crate) fn experimental_layer_pipeline(options: &CompilerOptions) -> PassPipeline {
    let mut pipeline = experimental_boundary_pipeline(options);
    pipeline.entries.push(PassEntry {
        pass: Pass::build(compile_instant_layers::CompileInstantLayers),
        enabled: options.optimization_level.unwrap_or(usize::MAX) >= 3,
    });
    pipeline
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_tiers_and_aliases_enable_certified_passes() {
        for flags in [
            "-O0",
            "-O1",
            "-O2",
            "-O3",
            "-O4",
            "-O5",
            "-O6",
            "-O",
            "-o",
            "--optimize",
            "-O5 --stable-instant-inputs",
            "-O6 --stable-instant-inputs",
            "-O --stable-instant-inputs",
            "-o --stable-instant-inputs",
            "--optimize --stable-instant-inputs",
        ] {
            let options = CompilerOptions::parse(flags).unwrap();
            let level = options.optimization_level.unwrap_or(crate::redpiler::MAX_OPTIMIZATION_LEVEL);
            assert_eq!(options.stable_instant_inputs, flags.contains("--stable-instant-inputs"));
            let pipeline = default_graph_pipeline(&options);
            let enabled: Vec<_> = pipeline.entries.iter().filter(|p| p.enabled).map(|p| p.pass.name()).collect();
            for (name, minimum) in [
                ("compile-fixed-classic", 2),
                ("compile-mixed-classic", 2),
                ("compile-wire-captures", 3),
                ("compile-classic-redstone", 2),
                ("experimental-compile-instant-wire-nodes", 4),
                ("experimental-minimize-wire-captures", 4),
                ("experimental-compile-bud-memory-reads", 4),
                ("experimental-compile-instant-events", 4),
                ("compile-logical-bud-owners", 6),
                ("optimize-instant-kernels-cpu", 5),
                ("compile-instant-transport", 5),
                ("compile-guarded-instant-transport", 5),
                ("compile-instant-drives", 5),
                ("compile-instant-transition-programs", 5),
                ("compile-instant-value-regions", 5),
                ("compile-signal-channels", 5),
                ("compile-stable-instant-logic", 5),
                ("compile-settled-circuit", 6),
                ("experimental-coalesce-delayed-updates", 5),
            ] {
                assert_eq!(enabled.contains(&name), level >= minimum, "{flags}: {name}");
            }
            assert_eq!(enabled.contains(&"compile-instant-layers"), level >= 6 && options.stable_instant_inputs);
            assert!(!enabled.contains(&"compile-guard-predicates"));
            assert!(
                enabled
                    .iter()
                    .all(|p| !p.starts_with("unsafe-") && *p != "experimental-settle-dust-networks")
            );
        }
        let options = CompilerOptions::parse("-O2").unwrap();
        assert!(
            default_graph_pipeline(&options)
                .entries
                .iter()
                .any(|p| p.enabled && p.pass.name() == "compile-classic-redstone")
        );
    }
    use crate::plot::PlotWorld;
    use crate::redpiler::components::{NodeState, NodeType};
    use crate::redpiler::program_graph::{CircuitNodeDraft, ComponentState, EntryState};
    use crate::world::World;
    use crate::world::storage::Chunk;
    use mchprs_blocks::blocks::{Block, Lever, LeverFace, RedstonePiston};
    use mchprs_blocks::{BlockDirection, BlockFace, BlockPos};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    struct TestPass {
        name: &'static str,
        strength: Option<u8>,
        reject: bool,
        fail: bool,
        cancel: bool,
        calls: Rc<Cell<(usize, usize)>>,
    }

    impl GraphPass for TestPass {
        fn name(&self) -> &'static str {
            self.name
        }

        fn validate_input(&self, _graph: &CompileGraph) -> Result<(), String> {
            let (validate, run) = self.calls.get();
            self.calls.set((validate + 1, run));
            if self.reject { Err("test precondition".into()) } else { Ok(()) }
        }

        fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
            let (validate, run) = self.calls.get();
            self.calls.set((validate, run + 1));
            if self.fail {
                return Err("test failure".into());
            }
            if let Some(strength) = self.strength {
                let ComponentState::Electrical(state) = &mut graph.nodes[0].state else {
                    panic!("test graph has an electrical node");
                };
                state.output_strength = strength;
            }
            if self.cancel {
                context.monitor.cancel();
            }
            Ok(())
        }
    }

    fn test_pass(name: &'static str, strength: Option<u8>, calls: Rc<Cell<(usize, usize)>>) -> TestPass {
        TestPass {
            name,
            strength,
            reject: false,
            fail: false,
            cancel: false,
            calls,
        }
    }

    fn graph_for_passes() -> CompileGraph {
        let block = Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        };
        CompileGraph {
            nodes: vec![CircuitNodeDraft {
                position: Some(BlockPos::new(1, 20, 1)),
                raw_state: Some(block.get_id()),
                kind: crate::redpiler::program_graph::CircuitNodeKind::Electrical(NodeType::Lever),
                state: ComponentState::Electrical(NodeState::default()),
                output_route: None,
            }],
            links: vec![],
            power_links: vec![],
            comparator_inputs: vec![],
            profiles: vec![],
            guards: vec![],
            contacts: vec![],
            routes: vec![],

            wires: vec![],

            wire_relations: vec![],
            sites: vec![],
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],

            plans: Default::default(),
        }
    }

    fn run_test_pipeline(graph: &mut CompileGraph, entries: Vec<PassEntry>, monitor: &TaskMonitor) -> Result<(), PassPipelineError> {
        let options = CompilerOptions::default();
        run_graph_pipeline(
            graph,
            &options,
            &EntryState {
                logical_tick: 0,
                pending_ticks: vec![],
            },
            &PassPipeline::new(entries),
            monitor,
        )
    }

    #[test]
    fn custom_parameterized_passes_run_in_supplied_order_and_can_be_disabled() {
        let standalone_calls = Rc::new(Cell::new((0, 0)));
        let mut standalone_graph = graph_for_passes();
        run_test_pipeline(
            &mut standalone_graph,
            vec![PassEntry {
                pass: Pass::rewrite(test_pass("standalone", Some(6), standalone_calls.clone())),
                enabled: true,
            }],
            &TaskMonitor::default(),
        )
        .unwrap();
        let ComponentState::Electrical(state) = &standalone_graph.nodes[0].state else {
            panic!("test graph has an electrical node");
        };
        assert_eq!(state.output_strength, 6);
        assert_eq!(standalone_calls.get(), (1, 1));

        let calls = Rc::new(Cell::new((0, 0)));
        let mut first = test_pass("set-low", Some(4), calls.clone());
        first.cancel = false;
        let second = test_pass("set-high", Some(9), calls.clone());
        let skipped = test_pass("skipped", Some(12), calls.clone());
        let mut graph = graph_for_passes();
        let monitor = TaskMonitor::default();
        run_test_pipeline(
            &mut graph,
            vec![
                PassEntry {
                    pass: Pass::rewrite(first),
                    enabled: true,
                },
                PassEntry {
                    pass: Pass::rewrite(skipped),
                    enabled: false,
                },
                PassEntry {
                    pass: Pass::rewrite(second),
                    enabled: true,
                },
            ],
            &monitor,
        )
        .unwrap();

        let ComponentState::Electrical(state) = &graph.nodes[0].state else {
            panic!("test graph has an electrical node");
        };
        assert_eq!(state.output_strength, 9);
        assert_eq!(calls.get(), (2, 2));
        let stats = monitor.graph_statistics();
        assert_eq!(
            stats.transformations.iter().map(|pass| pass.name).collect::<Vec<_>>(),
            ["set-low", "skipped", "set-high"]
        );
        assert_eq!(
            stats.passes.iter().map(|pass| pass.name).collect::<Vec<_>>(),
            ["set-low", "skipped", "set-high"]
        );
        assert!(!stats.passes[1].enabled);
        let summary = stats.summary_lines().join("\n");
        assert!(summary.contains("Transformations: 2 enabled, 1 skipped."));
        assert!(!summary.contains("optional optimization passes disabled"));
        assert_eq!(stats.passes[1].before, stats.passes[1].after);
        assert!(stats.passes[1].duration.is_zero());
        assert_eq!(stats.final_graph(), Some(GraphCounts::of(&graph)));

        let calls = Rc::new(Cell::new((0, 0)));
        let skipped = test_pass("all-disabled", Some(15), calls.clone());
        let before = graph.clone();
        let monitor = TaskMonitor::default();
        run_test_pipeline(
            &mut graph,
            vec![PassEntry {
                pass: Pass::rewrite(skipped),
                enabled: false,
            }],
            &monitor,
        )
        .unwrap();
        assert_eq!(graph, before);
        assert_eq!(calls.get(), (0, 0));
        assert_eq!(monitor.graph_statistics().final_graph(), Some(GraphCounts::of(&graph)));
        let summary = monitor.graph_statistics().summary_lines().join("\n");
        assert!(summary.contains("Transformations: 0 enabled, 1 skipped."));

        let monitor = TaskMonitor::default();
        run_test_pipeline(&mut graph, vec![], &monitor).unwrap();
        let statistics = monitor.graph_statistics();
        assert!(statistics.transformations.is_empty());
        assert!(statistics.summary_lines().iter().any(|line| line == "Transformations: none configured."));
    }

    #[test]
    fn cancelled_empty_and_all_disabled_pipelines_fail_before_validation() {
        for all_disabled in [false, true] {
            let calls = Rc::new(Cell::new((0, 0)));
            let entries = if all_disabled {
                vec![PassEntry {
                    pass: Pass::rewrite(test_pass("disabled", None, calls.clone())),
                    enabled: false,
                }]
            } else {
                vec![]
            };
            let mut graph = graph_for_passes();
            let ComponentState::Electrical(state) = &mut graph.nodes[0].state else {
                panic!("test graph has an electrical node");
            };
            state.output_strength = 16;
            let before = graph.clone();
            let monitor = TaskMonitor::default();
            monitor.cancel();

            assert!(matches!(
                run_test_pipeline(&mut graph, entries, &monitor),
                Err(PassPipelineError::Cancelled)
            ));
            assert_eq!(graph, before);
            assert_eq!(calls.get(), (0, 0));
            assert!(monitor.graph_statistics().passes.is_empty());
            assert_eq!(monitor.graph_statistics().final_graph(), None);
        }
    }

    #[test]
    fn removed_synthesis_is_absent_at_every_level_with_stable_inputs() {
        let synthesis = ["compile-symbolic-regions", "compile-instant-machine", "compile-symbolic-kernels"];
        for optimization_level in [Some(0), Some(3), Some(4), Some(5), Some(6), None] {
            for stable_instant_inputs in [false, true] {
                let options = CompilerOptions {
                    optimization_level,
                    stable_instant_inputs,
                    ..Default::default()
                };
                let mut graph = graph_for_passes();
                graph.guards.push(crate::redpiler::program_graph::Guard::Always);
                let monitor = TaskMonitor::default();
                run_graph_pipeline(
                    &mut graph,
                    &options,
                    &EntryState {
                        logical_tick: 0,
                        pending_ticks: vec![],
                    },
                    &default_graph_pipeline(&options),
                    &monitor,
                )
                .unwrap();
                let statistics = monitor.graph_statistics();
                for name in synthesis {
                    assert!(
                        statistics.passes.iter().all(|pass| pass.name != name),
                        "{name} must remain removed for {options:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn graph_pipeline_rejects_bad_names_preconditions_failures_and_malformed_output() {
        let empty_calls = Rc::new(Cell::new((0, 0)));
        let mut graph = graph_for_passes();
        assert!(matches!(
            run_test_pipeline(
                &mut graph,
                vec![PassEntry {
                    pass: Pass::rewrite(test_pass("", None, empty_calls.clone())),
                    enabled: true,
                }],
                &TaskMonitor::default(),
            ),
            Err(PassPipelineError::EmptyName)
        ));
        assert!(matches!(
            run_test_pipeline(
                &mut graph,
                vec![
                    PassEntry {
                        pass: Pass::rewrite(test_pass("same", None, empty_calls.clone())),
                        enabled: true,
                    },
                    PassEntry {
                        pass: Pass::rewrite(test_pass("same", None, empty_calls.clone())),
                        enabled: true,
                    },
                ],
                &TaskMonitor::default(),
            ),
            Err(PassPipelineError::DuplicateName("same"))
        ));

        let mut precondition = test_pass("precondition", None, empty_calls.clone());
        precondition.reject = true;
        assert!(matches!(
            run_test_pipeline(
                &mut graph,
                vec![PassEntry {
                    pass: Pass::rewrite(precondition),
                    enabled: true,
                }],
                &TaskMonitor::default(),
            ),
            Err(PassPipelineError::Precondition { pass: "precondition", .. })
        ));

        let mut failure = test_pass("failure", None, empty_calls.clone());
        failure.fail = true;
        let failure_monitor = TaskMonitor::default();
        assert!(matches!(
            run_test_pipeline(
                &mut graph,
                vec![PassEntry {
                    pass: Pass::rewrite(failure),
                    enabled: true,
                }],
                &failure_monitor,
            ),
            Err(PassPipelineError::Failed { pass: "failure", .. })
        ));
        let failed_pass = &failure_monitor.graph_statistics().passes[0];
        assert_eq!(failed_pass.name, "failure");
        assert!(failed_pass.enabled);
        assert_eq!(failed_pass.before, GraphCounts::of(&graph));
        assert_eq!(failed_pass.after, GraphCounts::of(&graph));

        let malformed = test_pass("malformed", Some(16), empty_calls);
        assert!(matches!(
            run_test_pipeline(
                &mut graph,
                vec![PassEntry {
                    pass: Pass::rewrite(malformed),
                    enabled: true,
                }],
                &TaskMonitor::default(),
            ),
            Err(PassPipelineError::MalformedOutput { pass: "malformed", .. })
        ));
    }

    #[test]
    fn graph_pipeline_checks_cancellation_between_passes_and_never_publishes_on_failure() {
        let calls = Rc::new(Cell::new((0, 0)));
        let mut cancel = test_pass("cancel", None, calls.clone());
        cancel.cancel = true;
        let second = test_pass("after-cancel", None, calls.clone());
        let mut graph = graph_for_passes();
        let monitor = TaskMonitor::default();
        assert!(matches!(
            run_test_pipeline(
                &mut graph,
                vec![
                    PassEntry {
                        pass: Pass::rewrite(cancel),
                        enabled: true,
                    },
                    PassEntry {
                        pass: Pass::rewrite(second),
                        enabled: true,
                    },
                ],
                &monitor,
            ),
            Err(PassPipelineError::Cancelled)
        ));
        assert_eq!(calls.get(), (1, 1));

        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let piston = BlockPos::new(4, 30, 4);
        world.set_block(
            piston,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFace::North.into(),
                    sticky: true,
                    extended: false,
                },
            },
        );
        let failure_calls = Rc::new(Cell::new((0, 0)));
        let mut failure = test_pass("compile-failure", None, failure_calls);
        failure.fail = true;
        let mut compiler = crate::redpiler::Compiler::default();
        let before_block = world.get_block(piston);
        let before_ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
        let before_tick = world.piston_state().logical_tick;
        let result = compiler.compile_with_graph_pipeline(
            &world,
            (BlockPos::new(0, 16, 0), BlockPos::new(15, 31, 15)),
            CompilerOptions::default(),
            vec![],
            Arc::default(),
            Some(PassPipeline::new(vec![PassEntry {
                pass: Pass::rewrite(failure),
                enabled: true,
            }])),
        );
        assert!(
            matches!(
                result,
                Err(crate::redpiler::CompileError::RichGraph(crate::redpiler::RichGraphError(
                    crate::redpiler::backend::graph::GraphBackendError::PassPipeline(PassPipelineError::Failed { pass: "compile-failure", .. })
                )))
            ),
            "unexpected compile result: {result:?}"
        );
        assert!(!compiler.is_active());
        assert!(compiler.stats().is_none());
        assert_eq!(world.get_block(piston), before_block);
        assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), before_ticks);
        assert_eq!(world.piston_state().logical_tick, before_tick);
    }
}
