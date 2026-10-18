//! Opt-in large-device layer census and independent native histories; no timing claims.
use super::*;
use crate::redpiler::backend::graph::GraphBackend;
use crate::redpiler::program_graph::*;
use crate::redpiler::{CompilerOptions, passes};

mod large_builds;

#[test]
#[ignore = "explicit Divider layer census and all 11 native differential histories"]
fn divider_layers_and_manifest_histories_match_native() {
    check_manifest_device("divider", &manifest("fpu_divider"), 8);
}

#[test]
#[ignore = "explicit COUNTER layer census and both native differential histories"]
fn counter_layers_and_manifest_histories_match_native() {
    let path = root().join("test_data/instant-pistons-io/fixtures/counter_basic.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    check_manifest_device("counter", &fixture, 1);
}

#[test]
#[ignore = "explicit revised PC-counter layer census and 128 frozen-reference ticks"]
fn pc_counter_layers_preserve_separate_controls_and_frozen_count_windows() {
    let fixture = pc_counter_contract::descriptor();
    let (mut native, bounds) = load(&fixture);
    let graph = prepare_layers(&native, bounds, 1);
    let mut report = layer_report("pc_counter", &graph);
    let mut backend = backend(&graph);
    let mut projected = load(&fixture).0;
    let controls = pc_counter_contract::controls(&native, bounds);
    assert_eq!(controls.len(), 23);
    for position in controls {
        lever_action(&mut native, position, true);
        backend.use_block(position).unwrap();
    }
    native.tick_interpreted();
    backend.tick().unwrap();
    backend.project_future(&mut projected);
    pc_counter_contract::compare_checkpoint(&native, &projected, bounds, "layered PC initial activation");
    for (index, expected) in pc_counter_contract::reference_words().into_iter().take(128).enumerate() {
        native.tick_interpreted();
        backend.tick().unwrap();
        project(&backend, &mut projected);
        assert_eq!(pc_counter_contract::output_word(&native), expected, "native frozen PC tick {}", index + 1);
        assert_eq!(
            pc_counter_contract::output_word(&projected),
            expected,
            "layered Graph PC tick {}",
            index + 1
        );
        assert_future_work(&native, &projected, "PC count window");
        if [1, 9, 15, 22, 64, 128].contains(&(index + 1)) {
            pc_counter_contract::compare_checkpoint(&native, &projected, bounds, &format!("layered PC tick {}", index + 1));
        }
    }
    check_handoff(&mut native, &mut backend, &mut projected, bounds, "PC");
    report["correctness"] = json!({"graph_native_ticks": 129, "frozen_output_ticks": 128, "independent_controls_enabled": 23, "handoff_ticks": 16});
    save_report("pc_counter", &report);
}

fn prepare_layers(world: &PlotWorld, bounds: (BlockPos, BlockPos), budget: usize) -> CompileGraph {
    let prepared = prepare_artifact(world, bounds, budget).unwrap();
    assert!(
        prepared.entry.pending_ticks.is_empty(),
        "these saved-entry fixtures carry no imported ticks"
    );
    assert_eq!(prepared.entry.logical_tick, 0);
    prepared.graph
}

fn prepare_artifact(world: &PlotWorld, bounds: (BlockPos, BlockPos), budget: usize) -> Result<passes::rich::PreparedGraph, String> {
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(budget);
    let ticks = world.scheduler().iter_entries().collect();
    let mut prepared =
        passes::rich::prepare(world, bounds, ticks, &monitor, AnalysisLimits::for_budget(budget)).map_err(|error| error.to_string())?;
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
    .map_err(|error| error.to_string())?;
    Ok(prepared)
}

fn backend(graph: &CompileGraph) -> GraphBackend {
    let (program, state) = graph.clone().lower().unwrap();
    let backend = GraphBackend::new(program, state, 0).unwrap();
    let coverage = backend.execution_coverage();
    assert_eq!(coverage.instant_kernel_nodes, 0, "a cold artifact cannot claim kernel execution");
    assert_eq!(coverage.native_nodes, 0);
    backend
}

fn layer_report(name: &str, graph: &CompileGraph) -> Value {
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let layers = graph.plans.instant.layers.as_ref().unwrap();
    let mut accounted_cells: Vec<_> = layers
        .regions
        .iter()
        .flat_map(|r| r.cells.iter().copied())
        .chain(layers.declines.iter().flat_map(|r| r.cells.iter().copied()))
        .collect();
    let mut accounted_corridors: Vec<_> = layers
        .regions
        .iter()
        .flat_map(|r| r.corridors.iter().copied())
        .chain(layers.declines.iter().flat_map(|r| r.corridors.iter().copied()))
        .collect();
    accounted_cells.sort_unstable();
    accounted_corridors.sort_unstable();
    assert_eq!(accounted_cells, (0..catalog.cells.len() as u32).collect::<Vec<_>>());
    assert_eq!(accounted_corridors, (0..catalog.corridors.len() as u32).collect::<Vec<_>>());
    let admitted_actors = catalog.cells.iter().map(|cell| cell.uses.len()).sum::<usize>();
    let mut all_actors: Vec<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter().map(|use_| use_.actor.index()))
        .chain(
            catalog
                .declines
                .iter()
                .flat_map(|decline| decline.actors.iter().map(|actor| actor.index())),
        )
        .collect();
    all_actors.sort_unstable();
    assert_eq!(
        all_actors,
        (0..graph.pistons.len()).collect::<Vec<_>>(),
        "every physical actor has one admission decision"
    );
    // Candidate occurrences include retries; count unique source actors separately.
    let compiled_actors = layers
        .regions
        .iter()
        .flat_map(|r| r.cells.iter())
        .flat_map(|&cell| catalog.cells[cell as usize].uses.iter().map(|use_| use_.actor))
        .collect::<BTreeSet<_>>()
        .len();
    let candidate_occurrences = layers.regions.iter().flat_map(|r| r.layers.iter()).map(|l| l.uses.len()).sum::<usize>();
    let mut admission_reasons = BTreeMap::<String, usize>::new();
    for decline in &catalog.declines {
        let reason = match decline.reason {
            CellDeclineReason::UnrecognizedActor(_) => "UnrecognizedActor",
            CellDeclineReason::DifferentPullSources(_) => "DifferentPullSources",
            CellDeclineReason::NoPullSource(_) => "NoPullSource",
            CellDeclineReason::MissingDeferredWake(_) => "MissingDeferredWake",
            CellDeclineReason::MissingResetWake(_) => "MissingResetWake",
            CellDeclineReason::PinTableBudgetExceeded => "PinTableBudgetExceeded",
            CellDeclineReason::ConflictingSourceContact(_) => "ConflictingSourceContact",
            CellDeclineReason::UnsupportedMaterial(_) => "UnsupportedMaterial",
        };
        *admission_reasons.entry(reason.to_owned()).or_default() += decline.actors.len();
    }
    let mut decline_reasons = BTreeMap::<String, usize>::new();
    let declines: Vec<_> = layers
        .declines
        .iter()
        .map(|decline| {
            *decline_reasons.entry(format!("{:?}", decline.reason)).or_default() += 1;
            let actors = decline.cells.iter().map(|&cell| catalog.cells[cell as usize].uses.len()).sum::<usize>();
            let power_cut = match decline.witness {
                LayerWitness::PowerAtCut { actor, link, phase } => json!({
                    "actor": actor.index(), "actor_position": graph.pistons[actor.index()].position,
                    "link": graph.power_links.iter().position(|original| *original == link).unwrap(),
                    "source_position": graph.sites[link.from.index()].position, "phase": format!("{phase:?}")
                }),
                _ => Value::Null,
            };
            json!({"cells": decline.cells.len(), "corridors": decline.corridors.len(), "actors": actors,
            "reason": format!("{:?}", decline.reason), "witness": format!("{:?}", decline.witness), "power_cut": power_cut})
        })
        .collect();
    let report = json!({"fixture":name, "total_actors":graph.pistons.len(), "admitted_actors":admitted_actors,
        "outside_actors":graph.pistons.len()-admitted_actors, "cells":catalog.cells.len(), "corridors":catalog.corridors.len(),
        "boundary_ports":catalog.ports.len(), "compiled_regions":layers.regions.len(), "compiled_actors":compiled_actors,
        "declined_regions":layers.declines.len(), "decline_reasons":decline_reasons, "declines":declines,
        "candidate_occurrences":candidate_occurrences, "admission_declined_actors":admission_reasons,
        "programs":layers.programs.iter().map(|program| json!({"inputs":program.inputs.len(),"expressions":program.expressions.len(),"roots":program.roots.len()})).collect::<Vec<_>>(),
        "runtime_replacement":false, "native_fallback":false});
    println!(
        "{name}: {admitted_actors}/{} admitted actors, {} cells, {} corridors, {compiled_actors} compiled actors, {} declined regions; {decline_reasons:?}",
        graph.pistons.len(),
        catalog.cells.len(),
        catalog.corridors.len(),
        layers.declines.len()
    );
    save_report(name, &report);
    report
}

fn save_report(name: &str, report: &Value) {
    let directory = root().join("target/layered-instant-devices");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(format!("{name}.json")), serde_json::to_vec_pretty(report).unwrap()).unwrap();
}

fn check_manifest_device(name: &str, fixture: &Value, budget: usize) {
    let (world, bounds) = load(fixture);
    let graph = match prepare_artifact(&world, bounds, budget) {
        Ok(prepared) => prepared.graph,
        Err(error) => {
            save_report(
                name,
                &json!({"fixture":name,"status":"preparation_rejected","error":error,"runtime_replacement":false}),
            );
            panic!("{name}: preparation rejected: {error}");
        }
    };
    let mut report = layer_report(name, &graph);
    drop(world);
    let mut ticks = 0;
    let cases = fixture["cases"].as_array().unwrap();
    assert!(!cases.is_empty(), "a manifest with no cases cannot establish execution correctness");
    for case in cases {
        let (mut native, _) = load(fixture);
        let mut projected = load(fixture).0;
        native.disable_command_output_limits_for_replay();
        projected.disable_command_output_limits_for_replay();
        let mut backend = backend(&graph);
        let positions = observation_positions(fixture);
        let mut steps = case.get("steps").or_else(|| case.get("actions")).unwrap().as_array().unwrap().clone();
        if let Some(tail) = case.get("ticks") {
            steps.push(json!({"advance": tail}));
        }
        for step in steps {
            if let Some(count) = step.get("advance") {
                for _ in 0..count.as_u64().unwrap() {
                    native.tick_interpreted();
                    backend.tick_with_world(&mut projected).unwrap();
                    project(&backend, &mut projected);
                    assert_observations(&native, &projected, &positions, case["id"].as_str().unwrap());
                    ticks += 1;
                }
            } else if let Some(diagnose) = step.get("diagnose") {
                // The manifest's diagnostic marker observes the checkpoint without an input event.
                assert_eq!(diagnose, true);
            } else {
                assert_eq!(step["op"], "lever", "unsupported manifest action cannot silently disappear");
                let position = local_pos(&step["pos"]) - BASE + origin(fixture);
                let powered = step["powered"].as_bool().unwrap();
                let Block::Lever { lever } = native.get_block(position) else {
                    panic!("missing control {position:?}");
                };
                let changed = lever.powered != powered;
                action(&mut native, fixture, &step);
                if changed {
                    backend.use_block(position).unwrap();
                }
            }
            project(&backend, &mut projected);
            pc_counter_contract::compare_checkpoint(&native, &projected, bounds, &format!("{name} {} operation {step}", case["id"]));
        }
        check_handoff(&mut native, &mut backend, &mut projected, bounds, &format!("{name} {}", case["id"]));
        println!("{name}: native differential history {} passed", case["id"]);
    }
    report["correctness"] = json!({"cases":fixture["cases"].as_array().unwrap().len(), "graph_native_ticks":ticks,
        "full_snapshot_after_every_operation":true, "handoff_ticks_per_case":16});
    save_report(name, &report);
}

fn observation_positions(fixture: &Value) -> Vec<BlockPos> {
    fn collect(value: &Value, offset: BlockPos, out: &mut Vec<BlockPos>) {
        if let Some(items) = value.as_array() {
            if items.len() == 3 && items.iter().all(Value::is_i64) {
                out.push(local_pos(value) - BASE + offset);
            } else {
                for item in items {
                    collect(item, offset, out);
                }
            }
        } else if let Some(fields) = value.as_object() {
            for value in fields.values() {
                collect(value, offset, out);
            }
        } else {
            panic!("unsupported observation shape {value}");
        }
    }
    let observations = fixture.get("observations").unwrap_or(&fixture["ports"]["observations"]);
    let mut positions = Vec::new();
    collect(observations, origin(fixture), &mut positions);
    assert!(!positions.is_empty());
    positions
}

fn project(backend: &GraphBackend, world: &mut PlotWorld) {
    world.clear_scheduled_ticks();
    world.piston_state_mut().motions.clear();
    backend.project_future(world);
}

fn assert_future_work(native: &PlotWorld, projected: &PlotWorld, context: &str) {
    assert_eq!(
        native.piston_state().logical_tick,
        projected.piston_state().logical_tick,
        "{context}: clock"
    );
    assert_eq!(native.piston_state().phase, projected.piston_state().phase, "{context}: phase");
    let expected: Vec<_> = native.scheduler().iter_entries().collect();
    let actual: Vec<_> = projected.scheduler().iter_entries().collect();
    let first_difference = expected
        .iter()
        .zip(&actual)
        .position(|(a, b)| a != b)
        .unwrap_or(expected.len().min(actual.len()));
    assert!(
        expected == actual,
        "{context}: pending deadlines/order differ at {first_difference}; counts native={} graph={}; native={:?} graph={:?}",
        expected.len(),
        actual.len(),
        expected.get(first_difference),
        actual.get(first_difference)
    );
}

fn assert_observations(native: &PlotWorld, projected: &PlotWorld, positions: &[BlockPos], context: &str) {
    for &position in positions {
        assert_eq!(
            native.get_block(position),
            projected.get_block(position),
            "{context}: output/memory at {position:?}"
        );
        assert_eq!(
            serde_json::to_value(native.get_block_entity(position)).unwrap(),
            serde_json::to_value(projected.get_block_entity(position)).unwrap(),
            "{context}: observed entity at {position:?}"
        );
    }
    assert_eq!(
        native.command_output().collect::<Vec<_>>(),
        projected.command_output().collect::<Vec<_>>(),
        "{context}: command output"
    );
    assert_future_work(native, projected, context);
}

fn check_handoff(native: &mut PlotWorld, backend: &mut GraphBackend, projected: &mut PlotWorld, bounds: (BlockPos, BlockPos), context: &str) {
    // Inspection already materialized Graph's future; replace it rather than importing it twice.
    projected.clear_scheduled_ticks();
    projected.piston_state_mut().motions.clear();
    backend.reset(projected);
    assert_future_work(native, projected, &format!("{context} handoff entry"));
    for tick in 1..=16 {
        native.tick_interpreted();
        projected.tick_interpreted();
        assert_future_work(native, projected, &format!("{context} handoff tick {tick}"));
    }
    pc_counter_contract::compare_checkpoint(native, projected, bounds, &format!("{context} interpreter handoff"));
}
