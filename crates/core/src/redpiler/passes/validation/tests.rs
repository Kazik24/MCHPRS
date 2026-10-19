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

/// The reference runner deliberately performs the former full output/exit checks.
fn fully_checked_pipeline(graph: &mut CompileGraph, options: &CompilerOptions, entry: &EntryState, pipeline: &passes::PassPipeline) {
    let monitor = TaskMonitor::default();
    graph.validate_with_monitor(&monitor).unwrap();
    let context = passes::PassContext {
        monitor: &monitor,
        options,
        entry,
    };
    for item in pipeline.entries.iter().filter(|item| item.enabled) {
        if matches!(item.pass, Pass::Rewrite(_)) {
            graph.clear_execution_plans();
        }
        item.pass.validate_input(graph).unwrap();
        item.pass.run(graph, &context).unwrap();
        graph.validate_with_monitor(&monitor).unwrap();
    }
    graph.validate_with_monitor(&monitor).unwrap();
}

#[test]
fn staged_validation_preserves_adder_graph_program_and_initial_state() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for fixture in ["adder_1bit", "adder_11bits"] {
        let path = root.join("test_data/instant-pistons-io/fixtures").join(format!("{fixture}.json"));
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let (world, _, dimensions) = crate::redstone::instant_piston_tests::load(&manifest, 0);
        let xyz = &manifest["coordinates"]["origin"];
        let origin = BlockPos::new(
            xyz[0].as_i64().unwrap() as i32,
            xyz[1].as_i64().unwrap() as i32,
            xyz[2].as_i64().unwrap() as i32,
        );
        let bounds = (
            origin,
            origin + BlockPos::new(dimensions.0 as i32 - 1, dimensions.1 as i32 - 1, dimensions.2 as i32 - 1),
        );
        let prepared = passes::rich::prepare(&world, bounds, vec![], &TaskMonitor::default(), AnalysisLimits::default()).unwrap();
        for level in [0, 2, 4] {
            let options = CompilerOptions {
                optimization_level: Some(level),
                stable_instant_inputs: true,
                ..Default::default()
            };
            let pipeline = passes::default_graph_pipeline(&options);
            let mut expected = prepared.graph.clone();
            fully_checked_pipeline(&mut expected, &options, &prepared.entry, &pipeline);
            let mut actual = prepared.graph.clone();
            passes::run_graph_pipeline(&mut actual, &options, &prepared.entry, &pipeline, &TaskMonitor::default()).unwrap();
            assert_eq!(actual, expected, "{fixture} O{level} prepared graph");
            let rebuilt_kernels = crate::redpiler::analysis::instant::layers::lower_kernels(&actual, &TaskMonitor::default()).unwrap();
            let lowered = actual.lower().unwrap();
            assert_eq!(lowered.0.instant_kernels(), rebuilt_kernels, "{fixture} O{level} reused layer analysis");
            assert_eq!(lowered, expected.lower().unwrap(), "{fixture} O{level} lowered program/state");
        }
    }
}
