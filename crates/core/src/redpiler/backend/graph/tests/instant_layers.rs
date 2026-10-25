//! The layer builder must account for ownership without silently activating runtime kernels.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::instant::layers::compile_layers;
use crate::redpiler::passes::compile_instant_layers::CompileInstantLayers;
use crate::redpiler::passes::planning::PlanBuilder;
use std::collections::BTreeSet;

#[path = "instant_layer_entry.rs"]
mod entry_tests;

fn prepare_layers(id: &'static str) -> passes::rich::PreparedGraph {
    let circuit = boundary_contract_tests::load_mechanism(id);
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&circuit.build(), circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::experimental_layer_pipeline(&options),
        &monitor,
    )
    .unwrap();
    prepared
}

#[test]
fn causal_layers_require_boundaries_and_cancellation_never_publishes() {
    let circuit = boundary_contract_tests::load_mechanism("adder_1bit");
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&circuit.build(), circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        stable_instant_inputs: true,
        ..Default::default()
    };
    let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
    assert!(CompileInstantLayers.build(&prepared.graph, &context).is_err());
    assert!(prepared.graph.plans.instant.layers.is_none());

    let catalog = crate::redpiler::analysis::instant::boundaries::infer_boundaries(&prepared.graph, &monitor).unwrap();
    prepared.graph.plans.instant.boundaries = Some(catalog);
    let before = prepared.graph.clone();
    let unstable_options = CompilerOptions::default();
    let unstable_context = passes::PassContext::new(&monitor, &unstable_options, &prepared.entry);
    assert!(
        CompileInstantLayers
            .run(&mut prepared.graph, &unstable_context)
            .unwrap_err()
            .contains("stable-input contract")
    );
    assert_eq!(prepared.graph, before, "an undeclared input protocol cannot publish a proof");
    monitor.cancel();
    assert!(CompileInstantLayers.run(&mut prepared.graph, &context).is_err());
    assert_eq!(prepared.graph, before, "cancellation must leave the previous graph intact");
}

#[test]
fn fixture_causal_layer_census_accounts_for_every_admitted_cell_once() {
    let directory = root().join("target/layered-instant-layers");
    std::fs::create_dir_all(&directory).unwrap();
    for id in super::instant_contract_tests::IO_FIXTURES {
        let prepared = prepare_layers(id);
        let graph = &prepared.graph;
        let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
        let plan = graph.plans.instant.layers.as_ref().unwrap();
        assert_eq!(
            plan.programs.len(),
            plan.regions.len(),
            "every admitted region publishes one reduced program"
        );
        assert_eq!(
            *plan,
            compile_layers(graph, catalog, &TaskMonitor::default()).unwrap(),
            "{id}: deterministic compilation"
        );
        let mut cells: Vec<_> = plan
            .regions
            .iter()
            .flat_map(|region| region.cells.iter().copied())
            .chain(plan.declines.iter().flat_map(|decline| decline.cells.iter().copied()))
            .collect();
        cells.sort_unstable();
        assert_eq!(
            cells,
            (0..catalog.cells.len() as u32).collect::<Vec<_>>(),
            "{id}: no omitted or duplicated cells"
        );
        let mut corridors: Vec<_> = plan
            .regions
            .iter()
            .flat_map(|region| region.corridors.iter().copied())
            .chain(plan.declines.iter().flat_map(|decline| decline.corridors.iter().copied()))
            .collect();
        corridors.sort_unstable();
        assert_eq!(
            corridors,
            (0..catalog.corridors.len() as u32).collect::<Vec<_>>(),
            "{id}: no omitted or duplicated corridors"
        );
        let admitted_actors = plan
            .regions
            .iter()
            .flat_map(|region| region.cells.iter())
            .map(|&cell| catalog.cells[cell as usize].uses.len())
            .sum::<usize>();
        let actor_set: BTreeSet<_> = plan
            .regions
            .iter()
            .flat_map(|region| region.cells.iter())
            .flat_map(|&cell| catalog.cells[cell as usize].uses.iter().map(|use_| use_.actor))
            .collect();
        assert_eq!(admitted_actors, actor_set.len(), "one computation-use owner per actor");
        let export = crate::redpiler::analysis::export_json::from_graph(&prepared.report, graph).unwrap();
        std::fs::write(directory.join(format!("{id}.graph.json")), export.to_bytes().unwrap()).unwrap();
        println!(
            "{id}: layered regions={}, compiled actors={admitted_actors}/{}, declines={:?}",
            plan.regions.len(),
            graph.pistons.len(),
            plan.declines
        );
    }
}

#[test]
fn causal_layer_certificate_requires_its_original_catalog() {
    let mut prepared = prepare_layers("adder_1bit");
    prepared.graph.plans.instant.boundaries = None;
    assert!(
        prepared.graph.validate().is_err(),
        "cold causal plans cannot outlive their boundary bindings"
    );
}

#[test]
fn causal_layer_building_is_read_only_and_lowering_checks_its_certificates() {
    let prepared = prepare_layers("adder_1bit");
    let mut source = prepared.graph.clone();
    let old_plan = source.plans.instant.layers.take().unwrap();
    let before = source.clone();
    let rebuilt = compile_layers(&source, source.plans.instant.boundaries.as_ref().unwrap(), &TaskMonitor::default()).unwrap();
    assert_eq!(source, before, "the builder only reads graph/state/route data");
    assert_eq!(rebuilt, old_plan);
    source.plans.instant.layers = Some(Default::default());
    assert!(source.validate().is_err(), "an empty replacement cannot erase source obligations");
    source.plans.instant.layers = Some(rebuilt);
    source.validate().unwrap();
    let lowered = source.lower().unwrap();
    let backend = GraphBackend::new_unactivated(lowered.0, lowered.1, 0).unwrap();
    assert!(
        backend.execution_coverage().instant_kernel_nodes > 0,
        "lowering installs the newly supported certificates"
    );
    assert_eq!(
        backend.execution_counts().instant_kernel_invocations,
        0,
        "construction does not execute the circuit"
    );
}

#[test]
fn region_partition_keeps_common_outside_inputs_separate() {
    let prepared = prepare_layers("adder_1bit");
    let graph = &prepared.graph;
    let mut catalog = graph.plans.instant.boundaries.as_ref().unwrap().clone();
    // This partition-only probe removes internal producers, retaining real original read
    // bindings. Shared outside inputs may never become an implicit ownership connection.
    catalog.ports = catalog
        .ports
        .iter()
        .copied()
        .filter(|port| port.direction == BoundaryDirection::Input)
        .collect();
    let partition = crate::redpiler::analysis::instant::regions::partition(graph, &catalog, &TaskMonitor::default()).unwrap();
    assert_eq!(partition.regions.len(), catalog.cells.len() + catalog.corridors.len());
    for region in &partition.regions {
        assert_eq!(region.cells.len() + region.corridors.len(), 1);
    }
}

#[test]
fn actual_adder_interfaces_join_cells_through_their_owned_corridors() {
    for id in ["adder_1bit", "adder_11bits"] {
        let prepared = prepare_layers(id);
        let graph = &prepared.graph;
        let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
        let partition = crate::redpiler::analysis::instant::regions::partition(graph, catalog, &TaskMonitor::default()).unwrap();
        assert!(
            partition
                .regions
                .iter()
                .any(|region| region.cells.len() > 1 && !region.corridors.is_empty())
        );
        let mut seen = BTreeSet::new();
        for region in &partition.regions {
            for &port in &region.ports {
                assert!(seen.insert(port), "a source interface belongs to exactly one connected region");
            }
        }
        assert_eq!(seen.len(), catalog.ports.len(), "all original interfaces retain provenance");
    }
}
