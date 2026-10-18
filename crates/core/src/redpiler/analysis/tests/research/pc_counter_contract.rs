//! Current PC-counter controls and frozen count windows; no schematic-specific optimizer.
use super::*;
use crate::redpiler::backend::graph::GraphBackend;
use crate::redpiler::{CompilerOptions, passes};
use mchprs_world::PistonAction;
use std::sync::Arc;

fn descriptor() -> Value {
    json!({
        "fixture": "test_data/piston-research/test-potados-counter-revised-20261008/TEST_POTADOS_PC_COUNTER.schem",
        "sha256": "641c50d1903ccf3715759007d5b82e0f04786020d8cce80fbdbb4a3c8a3596f7",
        "dimensions": [24, 19, 75],
        "origin": [40, 30, 40]
    })
}

fn controls(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> Vec<BlockPos> {
    let mut found = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let position = BlockPos::new(x, y, z);
                if let Block::Lever { lever } = world.get_block(position) {
                    assert!(!lever.powered, "saved PC control is OFF: {position:?}");
                    found.push(position);
                }
            }
        }
    }
    assert_eq!(found.len(), 23, "current fixture retains its 16 data and seven other controls");
    found
}

fn output_word(world: &PlotWorld) -> u16 {
    // The pinned reference defines OUT1/OUT2's active-low, MSB-first observation row.
    (0..16).fold(0, |word, bit| {
        let position = BASE + BlockPos::new(13, 2, 3 + 4 * bit);
        let Block::RedstoneRepeater { repeater } = world.get_block(position) else {
            panic!("missing declared PC output at {position:?}");
        };
        (word << 1) | u16::from(!repeater.powered)
    })
}

fn reference_words() -> Vec<u16> {
    let path = root().join("test_data/cpu-references/interpreter-20261009-pc-counter-reference.json");
    let report: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(report["backend"], "interpreted");
    assert_eq!(report["schematic_sha256"], descriptor()["sha256"]);
    let words: Vec<u16> = serde_json::from_value(report["per_game_tick_output_words"][0].clone()).unwrap();
    assert_eq!(words.len(), 4096);
    words
}

fn compile_pc(world: &PlotWorld, bounds: (BlockPos, BlockPos)) -> GraphBackend {
    GraphBackend::compile_with_options(
        world,
        bounds,
        vec![],
        &CompilerOptions {
            optimization_level: Some(3),
            ..Default::default()
        },
        Arc::default(),
    )
    .unwrap_or_else(|error| panic!("PC general Graph admission: {error}"))
}

#[test]
fn pc_counter_boundary_catalog_keeps_multiple_inputs_outputs_and_ordinary_memory_outside() {
    use crate::redpiler::program_graph::*;
    let (world, bounds) = load(&descriptor());
    let physical_controls = controls(&world, bounds);
    let monitor = crate::redpiler::TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::experimental_boundary_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let mut admitted: Vec<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter().map(|use_| use_.actor.index()))
        .collect();
    assert!(!admitted.is_empty());
    let admitted_count = admitted.len();
    assert!(!catalog.declines.is_empty(), "ordinary/BUD or unsupported shared groups stay outside");
    admitted.extend(
        catalog
            .declines
            .iter()
            .flat_map(|decline| decline.actors.iter().map(|actor| actor.index())),
    );
    admitted.sort_unstable();
    assert_eq!(admitted, (0..graph.pistons.len()).collect::<Vec<_>>());
    let mut direct_inputs = BTreeSet::new();
    let mut direct_outputs = BTreeSet::new();
    for port in &catalog.ports {
        if port.direction == BoundaryDirection::Input {
            let source = match port.kind {
                BoundaryPortKind::Callback { occurrence } => Some(graph.routes[occurrence.route.index()].source),
                BoundaryPortKind::WireNeighborhood { neighbor, .. } | BoundaryPortKind::CapturedWire { neighbor, .. } => {
                    match graph.sites[neighbor.index()].source {
                        SiteSource::Component(node) => Some(node),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(node) = source {
                if graph.nodes[node.index()]
                    .position
                    .is_some_and(|position| physical_controls.contains(&position))
                {
                    direct_inputs.insert(node);
                }
            }
        }
        if let BoundaryPortKind::Power { link } = port.kind {
            let link = graph.power_links[link as usize];
            match port.direction {
                BoundaryDirection::Input => {
                    if let SiteSource::Component(node) = graph.sites[link.from.index()].source {
                        if graph.nodes[node.index()]
                            .position
                            .is_some_and(|position| physical_controls.contains(&position))
                        {
                            direct_inputs.insert(node);
                        }
                    }
                }
                BoundaryDirection::Output => {
                    if let PowerTarget::Input { node, .. } = link.to {
                        direct_outputs.insert(node);
                    }
                }
            }
        }
    }
    assert!(direct_inputs.len() > 1, "the counter cannot be reduced to one triggering input");
    assert!(direct_outputs.len() > 1, "read/output devices have separate boundary targets");
    let directory = root().join("target/layered-instant-boundaries");
    std::fs::create_dir_all(&directory).unwrap();
    let export = crate::redpiler::analysis::export_json::from_graph(&prepared.report, graph).unwrap();
    // Compact serialization keeps this large inspection artifact within the existing viewer limit.
    // from_graph has already validated both the draft certificate and the exported records.
    let bytes = serde_json::to_vec(&export).unwrap();
    assert!(bytes.len() <= 64 * 1024 * 1024, "PC boundary export must fit the existing viewer limit");
    std::fs::write(directory.join("pc_counter.graph.json"), bytes).unwrap();
    println!(
        "PC boundaries: {} cells / {admitted_count} actors; {} outside groups; {} corridors; {} direct controls; {} direct output targets; {} total typed ports",
        catalog.cells.len(),
        catalog.declines.len(),
        catalog.corridors.len(),
        direct_inputs.len(),
        direct_outputs.len(),
        catalog.ports.len()
    );
}

fn compare_checkpoint(native: &PlotWorld, projected: &PlotWorld, bounds: (BlockPos, BlockPos), context: &str) {
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let position = BlockPos::new(x, y, z);
                assert_eq!(
                    projected.get_block(position),
                    native.get_block(position),
                    "PC {context}: memory/geometry at {position:?}"
                );
                assert_eq!(
                    serde_json::to_value(projected.get_block_entity(position)).unwrap(),
                    serde_json::to_value(native.get_block_entity(position)).unwrap(),
                    "PC {context}: entity at {position:?}"
                );
            }
        }
    }
    assert_eq!(
        projected.scheduler().iter_entries().collect::<Vec<_>>(),
        native.scheduler().iter_entries().collect::<Vec<_>>(),
        "PC {context}: ordered deadlines"
    );
}

/// Count accepted actions per real drain while preserving the distinction from samples.
fn check_native_drain(world: &mut PlotWorld) -> (usize, usize, usize, usize) {
    let entries = trace::capture(|| world.tick_interpreted());
    let mut retractions = BTreeSet::new();
    let (mut samples, mut pulls, mut extensions, mut drops) = (0, 0, 0, 0);
    for entry in entries {
        match entry.operation {
            trace::Operation::Sample { .. } => samples += 1,
            trace::Operation::Applied(event) => {
                assert_eq!(entry.phase, AdvancePhase::PistonEvents);
                match event.action {
                    PistonAction::Extend => extensions += 1,
                    PistonAction::Retract => {
                        assert!(
                            retractions.insert((event.pos.x, event.pos.y, event.pos.z)),
                            "second PC retraction in one drain: {event:?}"
                        );
                        pulls += 1;
                    }
                    PistonAction::RetractWithoutPull => drops += 1,
                }
            }
        }
    }
    (samples, pulls, extensions, drops)
}

#[test]
fn pc_counter_preserves_separate_controls_and_matches_native_output_windows() {
    let (mut native, bounds) = load(&descriptor());
    let controls = controls(&native, bounds);
    println!(
        "PC physical input positions: {:?}",
        controls.iter().map(|&position| position - BASE).collect::<Vec<_>>()
    );
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let position = BlockPos::new(x, y, z);
                if let Some(BlockEntity::Sign(sign)) = native.get_block_entity(position) {
                    println!("PC label {:?}: {:?}", position - BASE, sign.rows);
                }
            }
        }
    }
    let mut graph = compile_pc(&native, bounds);
    let mut projected = load(&descriptor()).0;
    for &position in &controls {
        lever_action(&mut native, position, true);
        graph.use_block(position).unwrap();
    }
    check_native_drain(&mut native);
    graph.tick().unwrap();
    let expected = reference_words();
    let mut totals = (0, 0, 0, 0);
    for (tick, &word) in expected.iter().take(128).enumerate() {
        let observed = check_native_drain(&mut native);
        totals.0 += observed.0;
        totals.1 += observed.1;
        totals.2 += observed.2;
        totals.3 += observed.3;
        graph.tick().unwrap();
        projected.clear_scheduled_ticks();
        projected.piston_state_mut().motions.clear();
        graph.project_future(&mut projected);
        assert_eq!(output_word(&native), word, "PC native frozen output tick {}", tick + 1);
        assert_eq!(output_word(&projected), word, "PC Graph frozen output tick {}", tick + 1);
        if [1, 9, 15, 22, 64, 128].contains(&(tick + 1)) {
            compare_checkpoint(&native, &projected, bounds, &format!("response tick {}", tick + 1));
        }
    }
    println!(
        "PC 128 ticks: samples={}, accepted retractions={}, accepted extensions={}, early-drops={}; 16 output bits matched native and frozen reference",
        totals.0, totals.1, totals.2, totals.3
    );
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&load(&descriptor()).0, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
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
    let input_nodes: BTreeSet<_> = controls
        .iter()
        .map(|position| {
            prepared
                .graph
                .nodes
                .iter()
                .position(|node| node.position == Some(*position))
                .expect("each physical PC control remains addressable")
        })
        .collect();
    assert_eq!(
        input_nodes.len(),
        23,
        "data and independent control ports are not collapsed into a common trigger"
    );
    let cells = crate::redpiler::analysis::instant::cells::discover_cells(&prepared.graph, &monitor).unwrap();
    println!(
        "PC stationary candidates: actors={}, cells={}, candidate actors={}, declined groups={:?}",
        prepared.graph.pistons.len(),
        cells.cells.len(),
        cells.cells.iter().map(|cell| cell.uses.len()).sum::<usize>(),
        cells.declined
    );
    let coverage = graph.execution_coverage();
    for line in coverage.summary_lines() {
        println!("PC after128 ticks: {line}");
    }
}

#[test]
fn pc_counter_independent_control_edges_preserve_memory_and_pending_work() {
    let (initial, bounds) = load(&descriptor());
    let inputs = controls(&initial, bounds);
    // Every saved control is stimulated separately; no assumption about a common trigger,
    // or a control's role inferred merely from the nearest sign, enters the oracle.
    for &input in &inputs {
        let (mut native, _) = load(&descriptor());
        let mut projected = load(&descriptor()).0;
        let mut graph = compile_pc(&native, bounds);
        for powered in [true, false] {
            lever_action(&mut native, input, powered);
            graph.use_block(input).unwrap();
            let context = format!("input {:?} = {powered}", input - BASE);
            compare_control_window(&mut native, &mut graph, &mut projected, bounds, &context);
        }
    }
    println!("PC independent controls: 23 rising and 23 falling histories; 736 ticks; full state and pending work at 138 checkpoints");
}

fn compare_control_window(native: &mut PlotWorld, graph: &mut GraphBackend, projected: &mut PlotWorld, bounds: (BlockPos, BlockPos), context: &str) {
    for tick in 1..=16 {
        check_native_drain(native);
        graph.tick().unwrap();
        projected.clear_scheduled_ticks();
        projected.piston_state_mut().motions.clear();
        graph.project_future(projected);
        let context = format!("{context}, tick {tick}");
        assert_eq!(output_word(projected), output_word(native), "PC {context}: output");
        if [1, 4, 16].contains(&tick) {
            compare_checkpoint(native, projected, bounds, &context);
        }
    }
}

#[test]
fn pc_counter_separate_controls_sample_two_stable_data_bus_patterns() {
    let (initial, bounds) = load(&descriptor());
    let inputs = controls(&initial, bounds);
    // This is the physical 16-bit input row, not a guess based on a nearby sign.
    let (data, separate): (Vec<_>, Vec<_>) = inputs.into_iter().partition(|position| {
        let local = *position - BASE;
        local.x == 23 && local.y == 10 && (5..=65).contains(&local.z) && (local.z - 5) % 4 == 0
    });
    assert_eq!(data.len(), 16);
    assert_eq!(separate.len(), 7);
    for pattern in [0x5555u16, 0xaaaau16] {
        for &input in &separate {
            let (mut native, _) = load(&descriptor());
            let mut projected = load(&descriptor()).0;
            let mut graph = compile_pc(&native, bounds);
            for (bit, &position) in data.iter().enumerate() {
                if pattern & (1 << bit) != 0 {
                    lever_action(&mut native, position, true);
                    graph.use_block(position).unwrap();
                }
            }
            let context = format!("stable data {pattern:04x}, independent control {:?}", input - BASE);
            compare_control_window(&mut native, &mut graph, &mut projected, bounds, &format!("{context}: data preparation"));
            for powered in [true, false] {
                lever_action(&mut native, input, powered);
                graph.use_block(input).unwrap();
                compare_control_window(&mut native, &mut graph, &mut projected, bounds, &format!("{context} = {powered}"));
            }
        }
    }
    println!("PC stable buses: two16-bit patterns ×seven separate controls; 672 ticks; full state and pending work at 126 checkpoints");
}

#[test]
#[ignore = "full 4096-tick native PC contract and frozen-reference validation"]
fn pc_counter_all_declared_count_windows_match_frozen_native_reference() {
    let (mut world, bounds) = load(&descriptor());
    for position in controls(&world, bounds) {
        lever_action(&mut world, position, true);
    }
    check_native_drain(&mut world);
    for (tick, expected) in reference_words().into_iter().enumerate() {
        check_native_drain(&mut world);
        assert_eq!(output_word(&world), expected, "PC frozen output tick {}", tick + 1);
    }
    assert_eq!(output_word(&world), 682);
}
