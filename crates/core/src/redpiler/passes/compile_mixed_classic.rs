//! Join fixed no-dust faces into one typed maximum inside mixed circuits.
//! Reuses the whole-fixed electrical compiler after fixed-signal preparation;
//! contacts, uncertain conductors and unsupported emitters retain per-face queries.
//! Controls whose general captured views retain live flags also stay general.
//! Queries are pure and read the same live/held state at the original seed or
//! Turbo operation, so regrouping max changes no wire writes, callbacks, piston
//! requests, captures, guards or future. No private wiring or actor is absorbed.
//! Discovery is linear in sites, queries and wires plus emitted operand count;
//! cancellation and the existing operand memory bound apply. Validation rederives
//! the plan; differential histories remain the independent behavioral check.
use super::{
    PassContext,
    planning::{PlanBuilder, PlanOutput},
};
use crate::redpiler::{
    PreparationTableStatistics,
    program_graph::{CompileGraph, MixedClassicProgram},
};

pub(crate) struct CompileMixedClassic;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::{
        CompilerOptions, TaskMonitor,
        analysis::AnalysisLimits,
        passes::{self, Pass, PassEntry, PassPipeline},
        program_graph::EntryState,
    };
    use crate::{
        plot::PlotWorld,
        world::{World, storage::Chunk},
    };
    use mchprs_blocks::{BlockPos, blocks::Block};

    #[test]
    fn mixed_builder_is_read_only_disabled_idempotent_and_invalidated_by_rewrites() {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(BlockPos::new(8, 19, 8), Block::Stone {});
        world.set_block(BlockPos::new(8, 20, 8), Block::RedstoneWire { wire: Default::default() });
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            vec![],
            &monitor,
            AnalysisLimits::default(),
        )
        .unwrap();
        let before = prepared.graph.clone();
        let options = CompilerOptions::default();
        let entry = EntryState {
            logical_tick: 0,
            pending_ticks: vec![],
        };
        for enabled in [false, true, true] {
            passes::run_graph_pipeline(
                &mut prepared.graph,
                &options,
                &entry,
                &PassPipeline::new(vec![PassEntry {
                    pass: Pass::build(CompileMixedClassic),
                    enabled,
                }]),
                &monitor,
            )
            .unwrap();
            let mut expected = before.clone();
            if enabled {
                expected.plans.mixed_classic = MixedClassicProgram::compile(&before, &monitor).unwrap();
            }
            assert_eq!(prepared.graph, expected);
        }
        let saved = prepared.graph.clone();
        monitor.cancel();
        assert!(
            CompileMixedClassic
                .build(
                    &prepared.graph,
                    &PassContext {
                        monitor: &monitor,
                        options: &options,
                        entry: &entry
                    }
                )
                .is_err()
        );
        assert_eq!(prepared.graph, saved);
        prepared.graph.clear_execution_plans();
        assert_eq!(prepared.graph, before);
    }
}

impl PlanBuilder for CompileMixedClassic {
    fn name(&self) -> &'static str {
        "compile-mixed-classic"
    }

    fn build(&self, graph: &CompileGraph, context: &PassContext<'_>) -> Result<PlanOutput, String> {
        let program = MixedClassicProgram::compile(graph, context.monitor)?;
        let faces = program.wires.iter().map(|wire| 6 - wire.general_faces.count_ones() as usize).sum();
        context.monitor.record_preparation_tables(vec![PreparationTableStatistics {
            stage: "mixed classic",
            table: "compiled no-dust faces".into(),
            entries: faces,
            capacity: faces,
            estimated_bytes: std::mem::size_of_val(program.wires.as_ref()) + std::mem::size_of_val(program.operands.as_ref()),
        }]);
        Ok(PlanOutput::MixedClassic(program))
    }
}
