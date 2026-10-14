//! Graph preparation pipelines; backends receive prepared execution data.
pub(crate) mod contract_dust_paths;
pub(crate) mod index_turbo_callbacks;
pub(crate) mod prune_inert_callbacks;
pub(crate) mod prune_unhandled_updates;
pub(crate) mod rich;
pub(crate) mod simplify_guards;
pub(crate) mod simplify_instant_resets;
pub(crate) mod simplify_state_guards;
pub(crate) mod specialize_wire_queries;
pub(crate) mod utils;

use super::program_graph::{CompileGraph, EntryState, GraphBuildError};
use super::{CompilerOptions, GraphCounts, PassStatistics, TaskMonitor};
use std::fmt;
use std::time::Instant;

/// A graph transformation with its own assumptions and optional parameters.
pub(crate) trait GraphPass {
    fn name(&self) -> &'static str;
    /// Check pass-specific preconditions; the runner already validates the graph.
    fn validate_input(&self, _graph: &CompileGraph) -> Result<(), String> {
        Ok(())
    }
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String>;

    /// Return true only when the pass keeps these derived records valid.
    fn preserves_dust_paths(&self) -> bool {
        false
    }

    /// Return true only when the pass keeps the callback indices valid.
    fn preserves_turbo_callback_indices(&self) -> bool {
        false
    }

    /// Preserve only when fixed site geometry and canonical bindings are unchanged.
    fn preserves_wire_query_facts(&self) -> bool {
        false
    }

    /// Preserve only when the private reset footprint, inputs and ordered routes stay valid.
    fn preserves_instant_resets(&self) -> bool {
        false
    }
}

#[allow(dead_code)]
/// Shared compilation inputs available to each graph transformation.
pub(crate) struct PassContext<'a> {
    /// This shares cancellation and progress reporting with compilation.
    pub(crate) monitor: &'a TaskMonitor,
    /// This exposes compiler settings without choosing which passes run.
    pub(crate) options: &'a CompilerOptions,
    /// This exposes entry time and pending work to transformation checks.
    pub(crate) entry: &'a EntryState,
}

/// A graph transformation and whether this pipeline should run it.
pub(crate) struct PassEntry {
    /// This owns a built-in or developer-defined transformation.
    pub(crate) pass: Box<dyn GraphPass>,
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
pub(crate) fn run_graph_pipeline(
    graph: &mut CompileGraph,
    options: &CompilerOptions,
    entry: &EntryState,
    pipeline: &PassPipeline,
    monitor: &TaskMonitor,
) -> Result<(), PassPipelineError> {
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
    graph.validate().map_err(PassPipelineError::InvalidInput)?;
    monitor.set_max_progress(monitor.progress() + pipeline.entries.len() + 1);
    let context = PassContext { monitor, options, entry };
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
        // Passes drop derived records by default and must opt in when they preserve their IDs.
        if !item.pass.preserves_dust_paths() && !graph.dust_paths.is_empty() {
            graph.dust_paths.clear();
        }
        if !item.pass.preserves_turbo_callback_indices() {
            graph.turbo_callback_indices.clear();
        }
        if !item.pass.preserves_wire_query_facts() {
            for wire in &mut graph.wires {
                wire.query_facts = None;
            }
        }
        if !item.pass.preserves_instant_resets() {
            for wire in &mut graph.wires {
                wire.instant_reset = None;
            }
        }
        monitor.set_message(name.to_string());
        let start = Instant::now();

        if let Err(message) = item.pass.validate_input(graph) {
            utils::record_transformation(monitor, name, before, GraphCounts::of(graph), start);
            return Err(PassPipelineError::Precondition { pass: name, message });
        }
        if let Err(message) = item.pass.run(graph, &context) {
            utils::record_transformation(monitor, name, before, GraphCounts::of(graph), start);
            return Err(PassPipelineError::Failed { pass: name, message });
        }
        if let Err(source) = graph.validate() {
            utils::record_transformation(monitor, name, before, GraphCounts::of(graph), start);
            return Err(PassPipelineError::MalformedOutput { pass: name, source });
        }

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

        utils::record_transformation(monitor, name, before, after, start);

        monitor.inc_progress();

        if monitor.cancelled() {
            return Err(PassPipelineError::Cancelled);
        }
    }
    if monitor.cancelled() {
        return Err(PassPipelineError::Cancelled);
    }
    graph.validate().map_err(PassPipelineError::InvalidInput)?;
    if monitor.cancelled() {
        return Err(PassPipelineError::Cancelled);
    }
    monitor.set_final_graph(GraphCounts::of(graph));
    Ok(())
}

/// Pass tiers are declared beside their implementations, ordered by minimum level.
pub(crate) fn default_graph_pipeline(options: &CompilerOptions) -> PassPipeline {
    let level = options.optimization_level.unwrap_or(usize::MAX);
    let passes: [(usize, Box<dyn GraphPass>); 8] = [
        (1, Box::new(simplify_guards::GuardAndUpdateReduction)),
        (1, Box::new(prune_inert_callbacks::PruneInertCallbacks)),
        (1, Box::new(prune_unhandled_updates::PruneUnhandledUpdates)),
        (1, Box::new(index_turbo_callbacks::IndexTurboCallbacks)),
        (1, Box::new(contract_dust_paths::ContractDustPaths)),
        (2, Box::new(simplify_state_guards::SimplifyStateGuards)),
        (2, Box::new(specialize_wire_queries::SpecializeWireQueries)),
        (3, Box::new(simplify_instant_resets::SimplifyInstantResets)),
    ];
    let entries = passes
        .into_iter()
        .map(|(minimum_level, pass)| PassEntry {
            pass,
            enabled: level >= minimum_level,
        })
        .collect();
    PassPipeline::new(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            turbo_callback_indices: vec![],
            wires: vec![],
            dust_paths: vec![],
            wire_relations: vec![],
            sites: vec![],
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],
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
                pass: Box::new(test_pass("standalone", Some(6), standalone_calls.clone())),
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
                    pass: Box::new(first),
                    enabled: true,
                },
                PassEntry {
                    pass: Box::new(skipped),
                    enabled: false,
                },
                PassEntry {
                    pass: Box::new(second),
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
                pass: Box::new(skipped),
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
                    pass: Box::new(test_pass("disabled", None, calls.clone())),
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
    fn graph_pipeline_rejects_bad_names_preconditions_failures_and_malformed_output() {
        let empty_calls = Rc::new(Cell::new((0, 0)));
        let mut graph = graph_for_passes();
        assert!(matches!(
            run_test_pipeline(
                &mut graph,
                vec![PassEntry {
                    pass: Box::new(test_pass("", None, empty_calls.clone())),
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
                        pass: Box::new(test_pass("same", None, empty_calls.clone())),
                        enabled: true,
                    },
                    PassEntry {
                        pass: Box::new(test_pass("same", None, empty_calls.clone())),
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
                    pass: Box::new(precondition),
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
                    pass: Box::new(failure),
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
                    pass: Box::new(malformed),
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
                        pass: Box::new(cancel),
                        enabled: true,
                    },
                    PassEntry {
                        pass: Box::new(second),
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
                pass: Box::new(failure),
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
