//! Fixed graph preparation pipeline for ordinary compilation.
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
mod unreachable_output;

use super::compile_graph::{CompileGraph, GraphError};
use super::{CompilerInput, CompilerOptions, GraphCounts, PassStatistics, TaskMonitor};
use crate::world::World;
use std::time::Instant;
use tracing::trace;

pub(super) fn run_passes<W: World>(
    options: &CompilerOptions,
    input: &CompilerInput<'_, W>,
    monitor: &TaskMonitor,
) -> Result<CompileGraph, GraphError> {
    prepare(options, input, monitor, false).map(|(graph, _)| graph)
}

pub(super) fn prepare<W: World>(
    options: &CompilerOptions,
    input: &CompilerInput<'_, W>,
    monitor: &TaskMonitor,
    allow_native: bool,
) -> Result<(CompileGraph, bool), GraphError> {
    let mut graph = CompileGraph::new();
    let native = std::cell::Cell::new(false);
    let pipeline_start = Instant::now();
    monitor.begin_graph_statistics(options.optimize);
    // Ten passes (including skipped ones), followed by backend compilation.
    monitor.set_max_progress(11);

    let mut run = |message: &'static str,
                   enabled: bool,
                   pass: &dyn Fn(&mut CompileGraph) -> Result<(), GraphError>| {
        let before = GraphCounts::of(&graph);
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
            after: GraphCounts::of(&graph),
            duration,
        });
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
    monitor.finish_graph_statistics(pipeline_start.elapsed());
    monitor.set_native_propagation(native.get());
    Ok((graph, native.get()))
}

pub(super) fn requires_native_propagation(graph: &CompileGraph) -> bool {
    // shortcut: retain all comparator selections, narrow this after broader ordering fuzzing.
    graph
        .node_weights()
        .any(|node| matches!(node.ty, super::compile_graph::NodeType::Comparator { .. }))
        || petgraph::algo::is_cyclic_directed(graph)
}

fn reject_comparator_ordering(graph: &CompileGraph) -> Result<(), GraphError> {
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
    use crate::redpiler::compile_graph::NodeType;
    use crate::world::storage::Chunk;
    use mchprs_blocks::blocks::{Block, Lever, LeverFace, RedstoneRepeater, RedstoneWire};
    use mchprs_blocks::{BlockDirection, BlockPos};

    #[test]
    fn feedback_guard_rejects_comparator_cycles_but_accepts_feedforward_logic() {
        use crate::redpiler::compile_graph::{
            CompileLink, CompileNode, LinkType, NodeState, NodeType,
        };

        for comparator in [false, true] {
            for cycle in [0, 1, 2] {
                let mut graph = CompileGraph::new();
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
                    let mut graph = CompileGraph::new();
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
            assert_eq!(statistics.final_graph().unwrap(), GraphCounts::of(&graph));
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
}
