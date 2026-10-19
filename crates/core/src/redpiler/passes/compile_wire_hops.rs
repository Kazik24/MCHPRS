//! Binds Turbo's existing canonical edges for direct hop execution.
//!
//! Each fixed wire root is bound to its component site; its existing 24 neighbor
//! IDs are the hop destinations, so no second adjacency table is retained.
//! Execution uses reusable dense site-to-slot scratch and captures only receiver,
//! profile and mutable properties. Contacts are still selected on first discovery,
//! and nested walks own separate scratch. Ordered queues, heading selection,
//! admission-time guards and intermediate writes remain the same state machine.
//!
//! The correctness relation maps each general walk's SiteId to the same captured
//! slot in the compiled walk. Every discovery, enqueue, write and delivery then
//! has the same operands and order; only address lookup and unused block views
//! disappear. No frozen topology or binary-input assumption is required. A missing
//! or ambiguous root declines specialization rather than changing admission.
//! Building is O(sites + wires), with one optional root ID retained per wire.
//! Runtime scratch is four bytes per program site per concurrent walk, allocated
//! lazily and reused; clearing touches only discovered sites, not the full graph.

use super::PassContext;
use super::planning::{PlanBuilder, PlanOutput};
use crate::redpiler::PreparationTableStatistics;
#[cfg(test)]
use crate::redpiler::passes::Pass;
use crate::redpiler::program_graph::{CompileGraph, SiteId, SiteSource};

/// Compiles wire roots without changing connectivity, state or callback routes.
pub(crate) struct CompileWireHops;

impl PlanBuilder for CompileWireHops {
    fn name(&self) -> &'static str {
        "compile-wire-hops"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let mut roots = vec![None; graph.nodes.len()];
        let mut ambiguous = vec![false; graph.nodes.len()];
        for (index, site) in graph.sites.iter().enumerate() {
            if context.monitor.cancelled() {
                return Err("cancelled while binding wire hop sites".into());
            }
            if let SiteSource::Component(node) = site.source {
                ambiguous[node.index()] |= roots[node.index()].is_some();
                roots[node.index()] = Some(SiteId::new(index as u32));
            }
        }
        if context.monitor.cancelled() {
            return Err("cancelled before publishing wire hops".into());
        }
        let bindings: Vec<_> = graph
            .wires
            .iter()
            .map(|wire| roots[wire.node.index()].filter(|_| !ambiguous[wire.node.index()]))
            .collect();
        let compiled = bindings.iter().flatten().count();
        context.monitor.record_preparation_tables(vec![PreparationTableStatistics {
            stage: "compiled wire hops",
            table: "bound wires".into(),
            entries: compiled,
            capacity: compiled,
            estimated_bytes: 0, // Root storage is included in the existing WirePlan table.
        }]);
        Ok(PlanOutput::WireHops(bindings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redpiler::passes::GraphPass;
    use crate::redpiler::program_graph::{EntryState, GraphBuildError};
    use crate::redpiler::{TaskMonitor, passes};
    use crate::world::{World, storage::Chunk};
    use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};

    fn graph() -> CompileGraph {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        for x in [6, 7] {
            let pos = BlockPos::new(x, 20, 5);
            world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(pos, Block::RedstoneWire { wire: Default::default() });
        }
        passes::rich::prepare(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            vec![],
            &TaskMonitor::default(),
            crate::redpiler::analysis::AnalysisLimits::default(),
        )
        .unwrap()
        .graph
    }

    fn compile_hops(graph: &mut CompileGraph) {
        CompileWireHops
            .run(
                graph,
                &PassContext::new(
                    &TaskMonitor::default(),
                    &Default::default(),
                    &EntryState {
                        logical_tick: 0,
                        pending_ticks: vec![],
                    },
                ),
            )
            .unwrap();
    }
    #[test]
    fn hop_roots_reject_dangling_or_wrong_bindings_and_decline_ambiguity() {
        let mut graph = graph();
        compile_hops(&mut graph);
        graph.validate().unwrap();
        let saved = graph.wires[0].execution.hop_root;
        for root in [SiteId::new(u32::MAX), graph.wires[1].execution.hop_root.unwrap()] {
            graph.wires[0].execution.hop_root = Some(root);
            assert!(matches!(graph.validate(), Err(GraphBuildError::InvalidWireHopRoot { wire: 0 })));
        }
        graph.wires[0].execution.hop_root = saved;
        graph.sites.push(graph.sites[saved.unwrap().index()]);
        assert!(matches!(graph.validate(), Err(GraphBuildError::InvalidWireHopRoot { wire: 0 })));
        compile_hops(&mut graph);
        assert_eq!(graph.wires[0].execution.hop_root, None);
    }

    #[test]
    fn disabled_passes_preserve_hops_and_enabled_unknown_passes_invalidate_them() {
        struct Identity;
        impl GraphPass for Identity {
            fn name(&self) -> &'static str {
                "identity"
            }
            fn run(&self, _graph: &mut CompileGraph, _context: &PassContext<'_>) -> Result<(), String> {
                Ok(())
            }
        }
        let mut graph = graph();
        compile_hops(&mut graph);
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        for enabled in [false, true] {
            let pipeline = passes::PassPipeline::new(vec![passes::PassEntry {
                pass: Pass::rewrite(Identity),
                enabled,
            }]);
            passes::run_graph_pipeline(&mut graph, &Default::default(), &entry, &pipeline, &TaskMonitor::default()).unwrap();
            assert!(graph.wires.iter().all(|wire| wire.execution.hop_root.is_some() != enabled));
        }
    }

    #[test]
    fn hop_pass_is_idempotent_and_cancelled_before_publication() {
        let mut graph = graph();
        compile_hops(&mut graph);
        let expected = graph.clone();
        compile_hops(&mut graph);
        assert_eq!(graph, expected);
        let monitor = TaskMonitor::default();
        monitor.cancel();
        let options = crate::redpiler::CompilerOptions::default();
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        let context = PassContext::new(&monitor, &options, &entry);
        assert!(passes::compile_wire_hops::CompileWireHops.run(&mut graph, &context).is_err());
        assert_eq!(graph, expected);
    }
}
