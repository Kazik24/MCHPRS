use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::{CompilerOptions, analysis::AnalysisLimits, passes, program_graph::*};
use crate::world::{World, storage::Chunk};
use mchprs_blocks::{BlockFace, BlockPos, blocks::Block};

fn prepare_test_circuit() -> passes::rich::PreparedGraph {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for (position, block) in [
        (BlockPos::new(5, 20, 5), Block::Lever { lever: Default::default() }),
        (BlockPos::new(6, 20, 5), Block::RedstoneWire { wire: Default::default() }),
        (BlockPos::new(7, 20, 5), Block::RedstoneLamp { lit: false }),
    ] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(position, block);
    }
    passes::rich::prepare(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
        vec![],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
}

struct BadIndex;
impl passes::planning::PlanBuilder for BadIndex {
    fn name(&self) -> &'static str {
        "bad-index"
    }
    fn build(&self, graph: &CompileGraph, _: &passes::PassContext<'_>) -> Result<passes::planning::PlanOutput, String> {
        Ok(passes::planning::PlanOutput::TurboCallbacks(vec![None; graph.wires.len() + 1]))
    }
}

struct ClearWireQueries;
impl passes::planning::PlanBuilder for ClearWireQueries {
    fn name(&self) -> &'static str {
        "clear-wire-queries"
    }
    fn build(&self, graph: &CompileGraph, _: &passes::PassContext<'_>) -> Result<passes::planning::PlanOutput, String> {
        Ok(passes::planning::PlanOutput::WireQueries(vec![None; graph.wires.len()]))
    }
}

#[test]
fn malformed_builder_output_is_rejected_at_its_own_stage() {
    let mut prepared = prepare_test_circuit();
    let pipeline = passes::PassPipeline::new(vec![passes::PassEntry {
        pass: Pass::build(BadIndex),
        enabled: true,
    }]);
    assert!(matches!(
        passes::run_graph_pipeline(
            &mut prepared.graph,
            &Default::default(),
            &prepared.entry,
            &pipeline,
            &TaskMonitor::default()
        ),
        Err(PassPipelineError::MalformedOutput {
            pass: "bad-index",
            source: GraphBuildError::StateTableLength { .. }
        })
    ));
}

#[test]
fn builder_rechecks_existing_plans_when_a_dependency_changes() {
    let mut prepared = prepare_test_circuit();
    let options = CompilerOptions {
        optimization_level: Some(2),
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &TaskMonitor::default(),
    )
    .unwrap();
    assert!(
        prepared.graph.plans.fixed_classic.program.is_some(),
        "fixture must own whole fixed execution"
    );
    let pipeline = passes::PassPipeline::new(vec![passes::PassEntry {
        pass: Pass::build(ClearWireQueries),
        enabled: true,
    }]);
    assert!(matches!(
        passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &TaskMonitor::default()),
        Err(PassPipelineError::MalformedOutput {
            pass: "clear-wire-queries",
            source: GraphBuildError::InvalidFixedClassic
        })
    ));
}

#[test]
fn empty_and_disabled_pipelines_still_reject_invalid_input() {
    for disabled in [false, true] {
        let mut prepared = prepare_test_circuit();
        prepared.graph.links[0].to = NodeId::new(u32::MAX);
        let entries = if disabled {
            vec![passes::PassEntry {
                pass: Pass::build(BadIndex),
                enabled: false,
            }]
        } else {
            vec![]
        };
        assert!(matches!(
            passes::run_graph_pipeline(
                &mut prepared.graph,
                &Default::default(),
                &prepared.entry,
                &passes::PassPipeline::new(entries),
                &TaskMonitor::default()
            ),
            Err(PassPipelineError::InvalidInput(_))
        ));
    }
}

#[test]
fn plan_validation_checks_wire_hop_bindings_against_the_structure() {
    let mut prepared = prepare_test_circuit();
    prepared.graph.validate().unwrap();
    prepared.graph.wires[0].execution.hop_root = Some(SiteId::new(u32::MAX));
    assert!(matches!(
        prepared.graph.validate_execution_plans_with_monitor(&TaskMonitor::default()),
        Err(GraphBuildError::InvalidWireHopRoot { wire: 0 })
    ));
}

struct MalformedPublication {
    /// The typed output family whose publication contract is exercised.
    kind: PlanChange,
    /// Number of proposed aligned rows, or repeated reset rows.
    rows: usize,
}

impl passes::planning::PlanBuilder for MalformedPublication {
    fn name(&self) -> &'static str {
        "malformed-publication"
    }
    fn build(&self, _: &CompileGraph, _: &passes::PassContext<'_>) -> Result<passes::planning::PlanOutput, String> {
        use passes::planning::PlanOutput;
        Ok(match self.kind {
            PlanChange::WireQueries => PlanOutput::WireQueries(vec![None; self.rows]),
            PlanChange::WireShapes => PlanOutput::WireShapes(vec![None; self.rows]),
            PlanChange::WireCallbacks => PlanOutput::WireCallbacks(vec![None; self.rows]),
            PlanChange::WireHops => PlanOutput::WireHops(vec![None; self.rows]),
            _ => unreachable!(),
        })
    }
}

#[test]
fn publication_rejects_truncated_oversized_and_out_of_range_rows_before_mutation() {
    for kind in [
        PlanChange::WireQueries,
        PlanChange::WireShapes,
        PlanChange::WireCallbacks,
        PlanChange::WireHops,
    ] {
        for rows in [0, 2, usize::MAX] {
            let mut prepared = prepare_test_circuit();
            assert_eq!(prepared.graph.wires.len(), 1);
            let original = prepared.graph.clone();
            let pipeline = passes::PassPipeline::new(vec![passes::PassEntry {
                pass: Pass::build(MalformedPublication { kind, rows }),
                enabled: true,
            }]);
            let result = passes::run_graph_pipeline(
                &mut prepared.graph,
                &Default::default(),
                &prepared.entry,
                &pipeline,
                &TaskMonitor::default(),
            );
            assert!(
                matches!(result, Err(PassPipelineError::MalformedOutput { .. })),
                "{kind:?} rows={rows}: {result:?}"
            );
            assert_eq!(prepared.graph, original, "invalid shape must never be published");
        }
    }
}

#[test]
fn selective_checks_match_full_checks_for_every_builder_pair() {
    let options = CompilerOptions {
        optimization_level: Some(4),
        ..Default::default()
    };
    let prepared = prepare_test_circuit();
    let monitor = TaskMonitor::default();
    let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
    let count = passes::default_graph_pipeline(&options).entries.len();
    for first in 0..count {
        for second in 0..count {
            if first == second {
                continue;
            }
            let all = passes::default_graph_pipeline(&options);
            if ![first, second]
                .iter()
                .all(|&i| all.entries[i].enabled && matches!(all.entries[i].pass, Pass::Build(_)))
            {
                continue;
            }
            let mut expected = prepared.graph.clone();
            let mut expected_result = Ok(());
            for &index in &[first, second] {
                let item = &all.entries[index];
                expected_result = item
                    .pass
                    .run(&mut expected, &context)
                    .and_then(|()| expected.validate().map_err(|error| error.to_string()));
                if expected_result.is_err() {
                    break;
                }
            }
            let mut entries = all.entries;
            let second_entry = entries.remove(second);
            let first_entry = entries.remove(if first > second { first - 1 } else { first });
            let names = [first_entry.pass.name(), second_entry.pass.name()];
            let pipeline = passes::PassPipeline::new(vec![first_entry, second_entry]);
            let mut actual = prepared.graph.clone();
            let actual_result = passes::run_graph_pipeline(&mut actual, &options, &prepared.entry, &pipeline, &monitor);
            assert_eq!(
                actual_result.is_ok(),
                expected_result.is_ok(),
                "{names:?}: selective={actual_result:?}, full={expected_result:?}"
            );
            if actual_result.is_ok() {
                assert_eq!(actual, expected, "{names:?}");
            }
        }
    }
}

#[test]
fn consuming_a_checked_result_discards_analysis_before_mutation() {
    let prepared = prepare_test_circuit();
    let checked = prepared
        .run_passes(&Default::default(), &passes::PassPipeline::default(), &TaskMonitor::default())
        .unwrap();
    let mut draft = checked.into_prepared();
    draft.graph.links[0].to = NodeId::new(u32::MAX);
    assert!(
        draft.lower(&TaskMonitor::default()).is_err(),
        "recovered mutable drafts must be fully admitted again"
    );
}
