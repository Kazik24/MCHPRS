//! Graph preparation pipelines; backends receive prepared execution data.
mod clamp_weights;
mod coalesce;
mod constant_coalesce;
mod constant_fold;
mod dedup_links;
mod export_graph;
mod identify_nodes;
mod input_search;
#[cfg(test)]
mod legacy_regressions;
mod prune_orphans;
pub(crate) mod rich;
mod unreachable_output;

pub(crate) use identify_nodes::identify_block;

use super::compile_graph::{GraphError, LegacyCompileGraph};
use super::program_graph::{CompileGraph, EntryState, GraphBuildError};
use super::{CompilerInput, CompilerOptions, GraphCounts, PassStatistics, TaskMonitor};
use crate::world::World;
use std::fmt;
use std::time::Instant;
use tracing::trace;

/// A graph transformation with its own assumptions and optional parameters.
pub(crate) trait GraphPass {
    fn name(&self) -> &'static str;
    fn validate_input(&self, graph: &CompileGraph) -> Result<(), String>;
    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String>;
}

#[allow(dead_code)]
pub(crate) struct PassContext<'a> {
    /// This shares cancellation and progress reporting with compilation.
    pub(crate) monitor: &'a TaskMonitor,
    /// This exposes compiler settings without choosing which passes run.
    pub(crate) options: &'a CompilerOptions,
    /// This exposes entry time and pending work to transformation checks.
    pub(crate) entry: &'a EntryState,
}

pub(crate) struct PassEntry {
    /// This owns a built-in or developer-defined transformation.
    pub(crate) pass: Box<dyn GraphPass>,
    /// This records whether the runner should validate and execute the pass.
    pub(crate) enabled: bool,
}

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
        Self {
            entries: Vec::new(),
        }
    }
}

#[derive(Debug)]
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
    MalformedOutput {
        pass: &'static str,
        source: GraphBuildError,
    },
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
    graph.validate().map_err(PassPipelineError::InvalidInput)?;
    monitor.set_max_progress(monitor.progress() + pipeline.entries.len() + 1);
    let context = PassContext {
        monitor,
        options,
        entry,
    };
    for item in &pipeline.entries {
        if monitor.cancelled() {
            return Err(PassPipelineError::Cancelled);
        }
        let name = item.pass.name();
        let before = GraphCounts::of(graph);
        if !item.enabled {
            monitor.record_pass(PassStatistics {
                name,
                enabled: false,
                before,
                after: before,
                duration: std::time::Duration::ZERO,
            });
            monitor.inc_progress();
            continue;
        }
        monitor.set_message(name.to_string());
        let start = Instant::now();
        if let Err(message) = item.pass.validate_input(graph) {
            record_failed_pass(monitor, name, before, graph, start);
            return Err(PassPipelineError::Precondition {
                pass: name,
                message,
            });
        }
        if let Err(message) = item.pass.run(graph, &context) {
            record_failed_pass(monitor, name, before, graph, start);
            return Err(PassPipelineError::Failed {
                pass: name,
                message,
            });
        }
        if let Err(source) = graph.validate() {
            record_failed_pass(monitor, name, before, graph, start);
            return Err(PassPipelineError::MalformedOutput { pass: name, source });
        }
        let after = GraphCounts::of(graph);
        monitor.record_pass(PassStatistics {
            name,
            enabled: true,
            before,
            after,
            duration: start.elapsed(),
        });
        monitor.inc_progress();
        if monitor.cancelled() {
            return Err(PassPipelineError::Cancelled);
        }
    }
    graph.validate().map_err(PassPipelineError::InvalidInput)?;
    monitor.set_final_graph(GraphCounts::of(graph));
    Ok(())
}

fn record_failed_pass(
    monitor: &TaskMonitor,
    name: &'static str,
    before: GraphCounts,
    graph: &CompileGraph,
    start: Instant,
) {
    monitor.record_pass(PassStatistics {
        name,
        enabled: true,
        before,
        after: GraphCounts::of(graph),
        duration: start.elapsed(),
    });
}

/// No rich rewrite has passed the behavioral review gate yet.
pub(crate) fn default_graph_pipeline(_options: &CompilerOptions) -> PassPipeline {
    PassPipeline::default()
}

pub(super) fn run_passes<W: World>(
    options: &CompilerOptions,
    input: &CompilerInput<'_, W>,
    monitor: &TaskMonitor,
) -> Result<LegacyCompileGraph, GraphError> {
    prepare(options, input, monitor, false).map(|(graph, _)| graph)
}

pub(super) fn prepare<W: World>(
    options: &CompilerOptions,
    input: &CompilerInput<'_, W>,
    monitor: &TaskMonitor,
    allow_native: bool,
) -> Result<(LegacyCompileGraph, bool), GraphError> {
    let mut graph = LegacyCompileGraph::new();
    let native = std::cell::Cell::new(false);
    let pipeline_start = Instant::now();
    monitor.begin_graph_statistics(options.optimize);
    // Ten passes (including skipped ones), followed by backend compilation.
    monitor.set_max_progress(11);

    let mut run =
        |message: &'static str,
         enabled: bool,
         pass: &dyn Fn(&mut LegacyCompileGraph) -> Result<(), GraphError>| {
            let before = GraphCounts::of_legacy(&graph);
            let duration = if enabled {
                if monitor.cancelled() {
                    return Err(GraphError::Cancelled);
                }
                trace!("Running pass: {message}");
                monitor.set_message(message.to_string());
                let start = Instant::now();
                pass(&mut graph)?;
                let duration = start.elapsed();
                trace!("Completed pass in {duration:?}");
                trace!("node_count: {}", graph.node_count());
                trace!("edge_count: {}", graph.edge_count());
                duration
            } else {
                trace!("Skipping pass: {message}");
                std::time::Duration::ZERO
            };
            monitor.record_pass(PassStatistics {
                name: message,
                enabled,
                before,
                after: GraphCounts::of_legacy(&graph),
                duration,
            });
            if message == "Clamping weights" {
                monitor.set_parser_complete(GraphCounts::of_legacy(&graph));
            }
            if message == "Exporting graph" {
                monitor.set_final_graph(GraphCounts::of_legacy(&graph));
            }
            monitor.inc_progress();
            Ok(())
        };

    // These three passes establish the nodes, inputs and valid link weights.
    run("Identifying nodes", true, &|graph| {
        identify_nodes::run(graph, options, input)
    })?;
    run("Searching for links", true, &|graph| {
        input_search::run(graph, input)
    })?;
    run("Clamping weights", true, &|graph| {
        clamp_weights::run(graph)?;
        if allow_native {
            native.set(requires_native_propagation(graph));
            Ok(())
        } else {
            reject_comparator_ordering(graph)
        }
    })?;

    // Keep a complete selection together: crossing executors would lose callback order.
    let optimize = options.optimize && !native.get();

    run("Deduplicating links", optimize, &dedup_links::run)?;
    run("Constant folding", optimize, &|graph| {
        constant_fold::run(graph, input.world)
    })?;
    run(
        "Pruning unreachable comparator outputs",
        optimize,
        &unreachable_output::run,
    )?;
    run("Coalescing constants", optimize, &constant_coalesce::run)?;
    run("Combining duplicate logic", optimize, &coalesce::run)?;
    run(
        "Pruning orphans",
        optimize && options.io_only,
        &prune_orphans::run,
    )?;
    run("Exporting graph", options.export, &|graph| {
        if native.get() {
            export_graph::validate(graph)?;
            return Err(GraphError::UnsupportedNativeExport);
        }
        export_graph::run(graph)
    })?;
    drop(run);
    monitor.finish_graph_statistics(pipeline_start.elapsed());
    monitor.set_native_propagation(native.get());
    Ok((graph, native.get()))
}

pub(super) fn requires_native_propagation(graph: &LegacyCompileGraph) -> bool {
    // shortcut: retain all comparator selections, narrow this after broader ordering fuzzing.
    graph
        .node_weights()
        .any(|node| matches!(node.ty, super::compile_graph::NodeType::Comparator { .. }))
        || petgraph::algo::is_cyclic_directed(graph)
}

fn reject_comparator_ordering(graph: &LegacyCompileGraph) -> Result<(), GraphError> {
    use super::compile_graph::NodeType;
    use petgraph::Direction;
    use rustc_hash::FxHashSet;

    if !graph
        .node_weights()
        .any(|node| matches!(node.ty, NodeType::Comparator { .. }))
    {
        return Ok(());
    }
    // Collapsed dust links cannot preserve native callback order around feedback.
    for component in petgraph::algo::kosaraju_scc(graph) {
        for &id in &component {
            if matches!(graph[id].ty, NodeType::Comparator { .. })
                && (component.len() > 1 || graph.contains_edge(id, id))
            {
                return Err(GraphError::UnsupportedComparatorFeedback {
                    pos: graph[id].block.expect("ordinary comparator has a block").0,
                });
            }
        }
    }
    // shortcut: reject direct shared-input forks; extend to deeper reconvergence if it diverges.
    for id in graph.node_indices() {
        if !matches!(graph[id].ty, NodeType::Comparator { .. }) {
            continue;
        }
        let inputs: FxHashSet<_> = graph.neighbors_directed(id, Direction::Incoming).collect();
        for &source in &inputs {
            if matches!(
                graph[source].ty,
                NodeType::Repeater { .. } | NodeType::Comparator { .. } | NodeType::Torch
            ) && graph
                .neighbors_directed(source, Direction::Incoming)
                .any(|shared| inputs.contains(&shared) && graph[shared].ty != NodeType::Constant)
            {
                return Err(GraphError::UnsupportedComparatorOrdering {
                    pos: graph[id].block.expect("ordinary comparator has a block").0,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redpiler::compile_graph::{NodeState, NodeType};
    use crate::redpiler::program_graph::{CircuitNodeDraft, ComponentState, EntryState};
    use crate::world::storage::Chunk;
    use mchprs_blocks::blocks::{
        Block, Lever, LeverFace, RedstonePiston, RedstoneRepeater, RedstoneWire,
    };
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
            if self.reject {
                Err("test precondition".into())
            } else {
                Ok(())
            }
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

    fn test_pass(
        name: &'static str,
        strength: Option<u8>,
        calls: Rc<Cell<(usize, usize)>>,
    ) -> TestPass {
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
        }
    }

    fn run_test_pipeline(
        graph: &mut CompileGraph,
        entries: Vec<PassEntry>,
        monitor: &TaskMonitor,
    ) -> Result<(), PassPipelineError> {
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
    fn feedback_guard_rejects_comparator_cycles_but_accepts_feedforward_logic() {
        use crate::redpiler::compile_graph::{
            CompileLink, CompileNode, LinkType, NodeState, NodeType,
        };

        for comparator in [false, true] {
            for cycle in [0, 1, 2] {
                let mut graph = LegacyCompileGraph::new();
                let node = |pos, ty| CompileNode {
                    ty,
                    block: Some((pos, 0)),
                    block_aliases: Vec::new(),
                    state: NodeState::default(),
                    is_input: false,
                    is_output: false,
                };
                let ty = if comparator {
                    NodeType::Comparator {
                        mode: mchprs_blocks::blocks::ComparatorMode::Subtract,
                        far_input: None,
                        facing_diode: false,
                    }
                } else {
                    NodeType::Repeater {
                        delay: 1,
                        facing_diode: false,
                    }
                };
                let first = graph.add_node(node(BlockPos::new(1, 30, 1), ty));
                let second = graph.add_node(node(BlockPos::new(2, 30, 1), NodeType::Torch));
                graph.add_edge(first, second, CompileLink::new(LinkType::Default, 0));
                if cycle == 1 {
                    graph.add_edge(first, first, CompileLink::new(LinkType::Side, 1));
                } else if cycle == 2 {
                    graph.add_edge(second, first, CompileLink::new(LinkType::Default, 0));
                }
                let result = reject_comparator_ordering(&graph);
                if comparator && cycle > 0 {
                    assert!(matches!(
                        result,
                        Err(GraphError::UnsupportedComparatorFeedback { .. })
                    ));
                } else {
                    result.unwrap();
                }
            }
        }
    }

    #[test]
    fn ordering_guard_rejects_shared_inputs_but_preserves_chains_and_constants() {
        use crate::redpiler::compile_graph::{CompileLink, CompileNode, LinkType, NodeState};
        let comparator = NodeType::Comparator {
            mode: mchprs_blocks::blocks::ComparatorMode::Subtract,
            far_input: None,
            facing_diode: false,
        };
        for timed in [
            comparator.clone(),
            NodeType::Repeater {
                delay: 1,
                facing_diode: false,
            },
            NodeType::Torch,
        ] {
            for shared_type in [NodeType::Lever, NodeType::Constant] {
                for fork in [false, true] {
                    let mut graph = LegacyCompileGraph::new();
                    let node = |pos, ty| CompileNode {
                        ty,
                        block: Some((pos, 0)),
                        block_aliases: Vec::new(),
                        state: NodeState::default(),
                        is_input: false,
                        is_output: false,
                    };
                    let dynamic = shared_type != NodeType::Constant;
                    let source = graph.add_node(node(BlockPos::new(1, 30, 1), shared_type.clone()));
                    let first = graph.add_node(node(BlockPos::new(2, 30, 1), timed.clone()));
                    let target = graph.add_node(node(BlockPos::new(3, 30, 1), comparator.clone()));
                    graph.add_edge(source, first, CompileLink::new(LinkType::Side, 0));
                    graph.add_edge(first, target, CompileLink::new(LinkType::Default, 0));
                    if fork {
                        graph.add_edge(source, target, CompileLink::new(LinkType::Side, 0));
                    }
                    let result = reject_comparator_ordering(&graph);
                    if fork && dynamic {
                        assert!(matches!(
                            result,
                            Err(GraphError::UnsupportedComparatorOrdering { .. })
                        ));
                    } else {
                        result.unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn pipeline_preserves_required_passes_flags_progress_and_cancellation() {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(
            BlockPos::new(4, 30, 4),
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        world.set_block(BlockPos::new(5, 30, 4), Block::RedstoneLamp { lit: false });
        world.set_block(
            BlockPos::new(3, 30, 4),
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: BlockDirection::East,
                    ..Default::default()
                },
            },
        );
        world.set_block(
            BlockPos::new(10, 30, 4),
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        let input = CompilerInput {
            world: &world,
            bounds: (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            ticks: &[],
        };
        for (optimize, io_only, expected_nodes) in [
            (false, false, 4),
            (false, true, 4),
            (true, false, 3),
            (true, true, 2),
        ] {
            let monitor = TaskMonitor::default();
            let options = CompilerOptions {
                optimize,
                io_only,
                ..Default::default()
            };
            let graph = run_passes(&options, &input, &monitor).unwrap();
            assert_eq!(graph.node_count(), expected_nodes);
            let lamp = graph
                .node_indices()
                .find(|&id| graph[id].ty == NodeType::Lamp)
                .unwrap();
            assert_eq!(
                graph
                    .neighbors_directed(lamp, petgraph::Direction::Incoming)
                    .count(),
                1
            );
            assert_eq!(monitor.progress(), 10);
            assert_eq!(monitor.max_progress(), 11);
            let statistics = monitor.graph_statistics();
            assert_eq!(statistics.passes.len(), 10);
            assert_eq!(statistics.wire_nodes_elided, optimize);
            assert_eq!(statistics.passes[0].before, GraphCounts::default());
            assert_eq!(
                statistics.baseline().unwrap().nodes,
                if optimize { 3 } else { 4 }
            );
            assert_eq!(
                statistics.final_graph().unwrap(),
                GraphCounts::of_legacy(&graph)
            );
            for pass in statistics.passes.iter().filter(|pass| !pass.enabled) {
                assert_eq!(pass.before, pass.after);
                assert!(pass.duration.is_zero());
            }
        }
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(matches!(
            run_passes(&CompilerOptions::default(), &input, &monitor),
            Err(GraphError::Cancelled)
        ));
        assert_eq!(monitor.progress(), 0);
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
            stats
                .passes
                .iter()
                .map(|pass| pass.name)
                .collect::<Vec<_>>(),
            ["set-low", "skipped", "set-high"]
        );
        assert!(!stats.passes[1].enabled);
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
        assert_eq!(
            monitor.graph_statistics().final_graph(),
            Some(GraphCounts::of(&graph))
        );
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
            Err(PassPipelineError::Precondition {
                pass: "precondition",
                ..
            })
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
            Err(PassPipelineError::Failed {
                pass: "failure",
                ..
            })
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
            Err(PassPipelineError::MalformedOutput {
                pass: "malformed",
                ..
            })
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
                Err(crate::redpiler::CompileError::RichGraph(
                    crate::redpiler::RichGraphError(
                        crate::redpiler::backend::graph::GraphBackendError::PassPipeline(
                            PassPipelineError::Failed {
                                pass: "compile-failure",
                                ..
                            }
                        )
                    )
                ))
            ),
            "unexpected compile result: {result:?}"
        );
        assert!(!compiler.is_active());
        assert!(compiler.stats().is_none());
        assert_eq!(world.get_block(piston), before_block);
        assert_eq!(
            world.scheduler().iter_entries().collect::<Vec<_>>(),
            before_ticks
        );
        assert_eq!(world.piston_state().logical_tick, before_tick);
    }
}
