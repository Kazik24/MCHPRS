//! Boundary behavior that a future instant pulse/oscillator replacement must preserve.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::backend::graph::GraphBackend;
use std::sync::Arc;

fn io_manifest(id: &str) -> Value {
    manifests_at(&root().join("test_data/instant-pistons-io"))
        .into_iter()
        .find(|manifest| manifest["id"] == id)
        .unwrap()
}

fn compile_at_level(world: &PlotWorld, bounds: (BlockPos, BlockPos), level: usize) -> GraphBackend {
    let options = CompilerOptions {
        optimization_level: Some(level),
        ..Default::default()
    };
    GraphBackend::compile_with_options(world, bounds, Vec::new(), &options, Arc::default()).unwrap()
}

#[test]
fn original_reset_without_feedback_holds_low_but_observer_chain_repeats() {
    for id in ["instant_reset_redstone", "instant_chain", "instant_blocked"] {
        let manifest = fixture(id);
        let mut case = named_case(&manifest, "falling");
        case["ticks"] = json!(96);
        let trace = episode(&manifest, &case, 0, "game", false);
        for tick in 1..=96 {
            let expected = match id {
                "instant_reset_redstone" => 0,
                "instant_chain" if tick % 6 == 0 => 15,
                "instant_chain" => 0,
                "instant_blocked" => 15,
                _ => unreachable!(),
            };
            assert_eq!(power(&trace["projections"][tick]["output"]), expected, "{id} tick {tick}");
        }
    }
}

#[test]
fn recurring_raw_output_does_not_make_its_repeater_a_delayed_copy() {
    for id in ["instant_reset_redstone", "instant_chain"] {
        let manifest = io_manifest(id);
        let mut case = named_case(&manifest, "activate");
        case["ticks"] = json!(96);
        let trace = episode(&manifest, &case, 0, "game", false);
        let projections = trace["projections"].as_array().unwrap();
        for (tick, value) in projections.iter().enumerate().skip(2) {
            assert_eq!(power(&value["raw_output"]), if (tick - 2) % 6 == 5 { 15 } else { 0 }, "{id} tick {tick}");
        }
        assert!(projections[11]["repeater"]["properties"]["powered"] == "true");
        assert!(projections[17]["repeater"]["properties"]["powered"] == "false");
        // A fixed delay of a six-tick periodic input must also repeat every six ticks.
        assert!(
            projections[8..90]
                .iter()
                .zip(&projections[14..96])
                .any(|(a, b)| { a["repeater"]["properties"]["powered"] != b["repeater"]["properties"]["powered"] })
        );
    }
}

#[test]
fn instant_io_graph_matches_interpreter_across_hold_release_and_retrigger_boundaries() {
    for id in ["instant_blocked", "instant_chain", "instant_reset_redstone"] {
        let manifest = io_manifest(id);
        for width in 0..=12 {
            for level in 0..=2 {
                let (mut native, positions, dimensions) = load(&manifest, 0);
                let (mut projected, _, _) = load(&manifest, 0);
                let low = positions.iter().copied().reduce(BlockPos::min).unwrap() - BlockPos::new(1, 1, 1);
                let high = positions.iter().copied().reduce(BlockPos::max).unwrap() + BlockPos::new(1, 1, 1);
                let mut graph = compile_at_level(&native, (low, high), level);
                let control = manifest["ports"]["inputs"]["trigger"].clone();
                let position = transform(triple(&control), dimensions, 0);
                for tick in 0..=48 {
                    // Release and retrigger at every offset across two nominal raw cycles.
                    let changes: &[bool] = if tick == 0 && width == 0 {
                        &[true, false]
                    } else if tick == 0 || tick == width + 12 {
                        &[true]
                    } else if tick == width {
                        &[false]
                    } else {
                        &[]
                    };
                    for &powered in changes {
                        mutate(&mut native, &json!({"op":"lever", "pos":control, "powered":powered}), dimensions, 0);
                        graph.use_block(position).unwrap();
                    }
                    native.tick_interpreted();
                    graph.tick().unwrap();
                    projected.clear_scheduled_ticks();
                    projected.piston_state_mut().motions.clear();
                    graph.project_future(&mut projected);
                    assert_eq!(
                        observe(&native, &manifest, dimensions, 0),
                        observe(&projected, &manifest, dimensions, 0),
                        "{id} O{level} width {width} tick {tick}"
                    );
                    let expected: Vec<_> = native.scheduler().iter_entries().collect();
                    let actual: Vec<_> = projected.scheduler().iter_entries().collect();
                    assert_eq!(expected, actual, "{id} width {width} tick {tick} pending ticks");
                    assert_eq!(
                        native.piston_state().events,
                        projected.piston_state().events,
                        "{id} width {width} tick {tick} pending actions"
                    );
                    for &position in &positions {
                        assert_eq!(
                            cell(&native, position),
                            cell(&projected, position),
                            "{id} width {width} tick {tick} cell {position:?}"
                        );
                    }
                }
            }
        }
    }
}
