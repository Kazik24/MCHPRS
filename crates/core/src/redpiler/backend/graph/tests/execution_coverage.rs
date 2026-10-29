//! Structural ownership reports the active physical Graph optimizations.
use super::*;
use crate::redpiler::GraphExecutionCoverage;

fn assert_partition(coverage: GraphExecutionCoverage) {
    assert_eq!(
        coverage.general_graph_nodes() + coverage.compiled_classic_wires + coverage.fixed_classic_nodes,
        coverage.component_nodes
    );
    assert!(coverage.compiled_classic_wires <= coverage.certified_classic_wires);
    assert!(coverage.certified_classic_wires <= coverage.wire_nodes);
}

#[test]
fn circuit_coverage_reports_physical_execution_ownership() {
    for id in ["adder_1bit", "adder_11bits"] {
        let circuit = boundary_contract_tests::load_mechanism(id);
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(&circuit.build(), circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
        let options = crate::redpiler::CompilerOptions {
            optimization_level: Some(3),
            ..Default::default()
        };
        passes::run_graph_pipeline(
            &mut prepared.graph,
            &options,
            &prepared.entry,
            &passes::default_graph_pipeline(&options),
            &monitor,
        )
        .unwrap();
        let (program, state) = prepared.graph.lower().unwrap();
        let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
        let coverage = backend.execution_coverage();
        assert_partition(coverage);
        println!(
            "{id}: physical Graph ownership: {} general nodes, {} classic wires",
            coverage.general_graph_nodes(),
            coverage.compiled_classic_wires
        );
        for line in coverage.summary_lines() {
            println!("  {line}");
        }

        // A deoptimized classic net returns to general Graph ownership.
        backend.state.classic_active.fill(false);
        let deoptimized = backend.execution_coverage();
        assert_partition(deoptimized);
        assert_eq!(deoptimized.compiled_classic_wires, 0);
        assert_eq!(deoptimized.general_graph_nodes(), deoptimized.component_nodes);
        assert_eq!(deoptimized.certified_classic_wires, coverage.certified_classic_wires);
    }
}

#[test]
fn empty_circuit_coverage_uses_not_applicable_percentages() {
    let coverage = GraphExecutionCoverage::default();
    assert_partition(coverage);
    let summary = coverage.summary_lines().join("\n");
    assert!(summary.contains("0/0 (n/a)"));
    assert!(!summary.contains("NaN"));
}

#[test]
fn active_classic_wire_coverage_returns_to_graph_after_deoptimization() {
    use mchprs_blocks::BlockDirection;
    use mchprs_blocks::blocks::{Lever, RedstoneRepeater};
    let mut world = empty_world();
    for x in 0..=5 {
        world.set_block(ORIGIN + BlockPos::new(x, -1, 0), Block::Stone {});
    }
    world.set_block(
        ORIGIN,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    for x in 1..=4 {
        world.set_block(ORIGIN + BlockPos::new(x, 0, 0), Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(
        ORIGIN + BlockPos::new(5, 0, 0),
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::West,
                delay: 2,
                powered: false,
                locked: false,
            },
        },
    );
    for x in 1..=4 {
        let position = ORIGIN + BlockPos::new(x, 0, 0);
        let wire = redstone::wire::get_state_for_placement(&world, position);
        world.set_block(position, Block::RedstoneWire { wire });
    }
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(2),
        ..Default::default()
    };
    let mut pipeline = passes::default_graph_pipeline(&options);
    pipeline
        .entries
        .retain(|entry| !matches!(&entry.pass, passes::Pass::Build(builder) if builder.name() == "compile-fixed-classic"));
    let mut backend = GraphBackend::compile_with_pipeline(
        &world,
        (ORIGIN - BlockPos::new(1, 1, 1), ORIGIN + BlockPos::new(6, 2, 1)),
        vec![],
        &options,
        &pipeline,
        Arc::default(),
    )
    .unwrap();
    let active = backend.execution_coverage();
    assert_partition(active);
    assert_eq!(active.compiled_classic_wires, 4);
    assert_eq!(active.wire_nodes, 4);
    assert_eq!(active.component_nodes, 6);
    assert!(active.summary_lines()[1].contains("4/4 (100.0%)"));
    for net in 0..backend.program.classic().nets.len() {
        backend.deopt_classic_net(net);
    }
    let general = backend.execution_coverage();
    assert_partition(general);
    assert_eq!(general.compiled_classic_wires, 0);
    assert_eq!(general.certified_classic_wires, 4);
    assert_eq!(general.general_graph_nodes(), 6);
    assert!(general.summary_lines()[0].contains("6/6 (100.0%)"));
}
