//! Fixed graph preparation pipeline, shared by ordinary and instant compilation.
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
    let mut graph = CompileGraph::new();
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
    run("Clamping weights", true, &clamp_weights::run)?;

    run("Deduplicating links", options.optimize, &dedup_links::run)?;
    run("Constant folding", options.optimize, &|graph| {
        constant_fold::run(graph, input.world)
    })?;
    run(
        "Pruning unreachable comparator outputs",
        options.optimize,
        &unreachable_output::run,
    )?;
    run(
        "Coalescing constants",
        options.optimize,
        &constant_coalesce::run,
    )?;
    run(
        "Combining duplicate logic",
        options.optimize,
        &coalesce::run,
    )?;
    run(
        "Pruning orphans",
        options.optimize && options.io_only,
        &prune_orphans::run,
    )?;
    run("Exporting graph", options.export, &export_graph::run)?;
    monitor.finish_graph_statistics(pipeline_start.elapsed());
    Ok(graph)
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
            boundaries: None,
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
