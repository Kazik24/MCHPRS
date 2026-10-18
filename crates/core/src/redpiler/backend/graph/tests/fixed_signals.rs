//! Weighted tracing must preserve channels, attenuation and undelivered live dust.
use super::*;
use crate::plot::PlotWorld;
use crate::world::storage::Chunk;
use mchprs_blocks::{BlockDirection, blocks::RedstoneComparator};
use std::collections::BTreeMap;

const A: BlockPos = BlockPos::new(5, 20, 5);
const B: BlockPos = BlockPos::new(8, 20, 5);
const RECEIVER: BlockPos = BlockPos::new(6, 20, 6);
const WIRES: [BlockPos; 3] = [BlockPos::new(6, 20, 5), BlockPos::new(7, 20, 5), BlockPos::new(7, 20, 6)];

fn fixture() -> CompileGraph {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for (position, facing) in [(A, BlockDirection::West), (B, BlockDirection::East), (RECEIVER, BlockDirection::North)] {
        world.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(facing, ComparatorMode::Compare, false),
            },
        );
        world.set_block_entity(position, BlockEntity::Comparator { output_strength: 0 });
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    for position in WIRES {
        world.set_block(position, Block::RedstoneWire { wire: Default::default() });
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    for position in WIRES {
        let wire = crate::redstone::wire::get_state_for_placement(&world, position);
        world.set_block(position, Block::RedstoneWire { wire });
    }
    passes::rich::prepare(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
        vec![],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
    .graph
}

fn backend(graph: CompileGraph) -> GraphBackend {
    let (program, state) = graph.lower().unwrap();
    GraphBackend::new(program, state, 0).unwrap()
}

fn strength(backend: &mut GraphBackend, position: BlockPos, strength: u8) {
    let node = backend.positions[&position];
    match &mut backend.state.components[node.index()] {
        ComponentState::Electrical(state) => {
            state.output_strength = strength;
            state.powered = strength != 0;
        }
        ComponentState::Wire(wire) => wire.power = strength,
        _ => panic!("expected a signal owner"),
    }
}

fn query(backend: &GraphBackend, target: PowerTarget) -> u8 {
    match target {
        PowerTarget::Input { node, port } => backend.input_strength(node, port),
        PowerTarget::Conductor { site, include_dust } => backend.site_power(site, PowerPort::Conducted { include_dust }),
    }
}

#[test]
fn tracer_recovers_multi_ingress_main_and_side_weights() {
    let graph = fixture();
    let plans = infer_fixed_signals(&graph, &TaskMonitor::default()).unwrap();
    let node = |position| NodeId::new(graph.nodes.iter().position(|node| node.position == Some(position)).unwrap() as u32);
    for (port, losses) in [(InputPort::Main, [0, 1]), (InputPort::Side, [2, 1])] {
        let query = plans
            .traced
            .iter()
            .find(|query| query.target == PowerTarget::Input { node: node(RECEIVER), port })
            .unwrap();
        let expected = BTreeMap::from([(node(A), losses[0]), (node(B), losses[1])]);
        let actual = query
            .inputs
            .iter()
            .map(|input| {
                assert!(!input.require_cross);
                (input.node, input.attenuation)
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(actual, expected, "{port:?}");
        assert_eq!(query.constant, 0);
    }
}

#[test]
fn compiled_live_queries_preserve_unsettled_analog_wire_state() {
    let graph = fixture();
    let targets = graph.power_links.iter().map(|link| link.to).collect::<std::collections::BTreeSet<_>>();
    let mut compiled = graph.clone();
    compiled.plans.fixed_signals = infer_fixed_signals(&compiled, &TaskMonitor::default()).unwrap();
    assert!(compiled.plans.fixed_signals.fallback.is_empty());
    let mut general = backend(graph);
    let mut compiled = backend(compiled);
    for a in 0..16u8 {
        for b in 0..16u8 {
            for old in 0..16u8 {
                for backend in [&mut general, &mut compiled] {
                    strength(backend, A, a);
                    strength(backend, B, b);
                    for (index, position) in WIRES.into_iter().enumerate() {
                        strength(backend, position, (old + index as u8 * 5) % 16);
                    }
                }
                for target in &targets {
                    assert_eq!(query(&compiled, *target), query(&general, *target), "{target:?} A={a} B={b} old={old}");
                }
                let receiver = compiled.positions[&RECEIVER];
                assert_eq!(compiled.input_strength(receiver, InputPort::Main), old);
                assert_eq!(compiled.input_strength(receiver, InputPort::Side), (old + 10) % 16);
            }
        }
    }
}

#[test]
fn validator_rejects_a_well_formed_but_wrong_weight() {
    let mut graph = fixture();
    graph.plans.fixed_signals = infer_fixed_signals(&graph, &TaskMonitor::default()).unwrap();
    graph.validate().unwrap();
    let query = graph.plans.fixed_signals.live.iter_mut().find(|query| !query.inputs.is_empty()).unwrap();
    query.inputs[0].attenuation = (query.inputs[0].attenuation + 1) % 15;
    assert!(matches!(graph.validate(), Err(GraphBuildError::InvalidFixedSignals)));
}

#[test]
fn captured_site_guard_is_false_for_a_live_power_query() {
    let mut graph = fixture();
    let a = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(A)).unwrap() as u32);
    let link = graph
        .power_links
        .iter()
        .position(|link| graph.sites[link.from.index()].source == SiteSource::Component(a))
        .unwrap();
    let target = graph.power_links[link].to;
    let guard = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Site {
        site: graph.power_links[link].from,
        view: OperandView::Captured,
        test: SiteTest::Transparent,
    });
    graph.power_links[link].guard = guard;
    let mut compiled = graph.clone();
    compiled.plans.fixed_signals = infer_fixed_signals(&compiled, &TaskMonitor::default()).unwrap();
    let mut general = backend(graph);
    let mut compiled = backend(compiled);
    for value in 0..16 {
        strength(&mut general, A, value);
        strength(&mut compiled, A, value);
        assert_eq!(query(&compiled, target), query(&general, target));
    }
}

#[test]
#[ignore = "research: paired full-CHUNGUS query execution, with complete state and FIFO checks"]
fn chungus_fixed_queries_paired_execution() {
    use std::time::{Duration, Instant};
    let ticks = std::env::var("MCHPRS_CHUNGUS_TICKS")
        .ok()
        .map(|value| value.parse::<usize>().unwrap())
        .unwrap_or(4096);
    let comparison = std::env::var("MCHPRS_CHUNGUS_COMPARE").unwrap_or_else(|_| "queries".to_owned());
    assert!(matches!(comparison.as_str(), "queries" | "pending" | "callbacks" | "combined"));
    let world = super::chungus_census::load_chungus_world();
    let monitor = Arc::new(TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(2),
        budget_multiplier: 8,
        ..Default::default()
    };
    let start = Instant::now();
    let compiled = GraphBackend::compile_without_fixed_classic(
        &world,
        world.get_corners(),
        world.scheduler().iter_entries().collect(),
        &options,
        monitor.clone(),
    )
    .unwrap();
    let compile_seconds = start.elapsed().as_secs_f64();
    let mut general_program = compiled.program.clone();
    if matches!(comparison.as_str(), "queries" | "combined") {
        general_program = general_program.without_fixed_signal_queries();
    }
    if matches!(comparison.as_str(), "callbacks" | "combined") {
        general_program = general_program.without_fixed_wire_callbacks();
    }
    let mut general = GraphBackend::new(general_program, compiled.state.clone(), 0).unwrap();
    general.test_recorder.general_pending_queries = matches!(comparison.as_str(), "pending" | "combined");
    let mut engines = [general, compiled];
    let mut activation = [Duration::ZERO; 2];
    let mut execution = [Duration::ZERO; 2];
    for (index, engine) in engines.iter_mut().enumerate() {
        let start = Instant::now();
        engine.use_block(super::chungus_census::START_BUTTON).unwrap();
        activation[index] = start.elapsed();
    }
    for tick in 0..=ticks {
        if tick > 0 {
            for index in if tick % 2 == 0 { [0, 1] } else { [1, 0] } {
                let start = Instant::now();
                engines[index].tick().unwrap();
                execution[index] += start.elapsed();
            }
        }
        let [general, compiled] = &engines;
        assert!(general.state == compiled.state, "complete circuit state at tick {tick}");
        assert!(general.scheduled_ticks() == compiled.scheduled_ticks(), "ordered future at tick {tick}");
        assert!(general.future.pending == compiled.future.pending, "pending membership at tick {tick}");
        assert!(
            general.future.pending_nodes == compiled.future.pending_nodes,
            "dense pending multiplicity at tick {tick}"
        );
        assert_eq!(general.future.clock.logical_tick, compiled.future.clock.logical_tick);
        assert_eq!(general.future.clock.phase, compiled.future.clock.phase);
    }
    let statistics = monitor.graph_statistics();
    let tables = statistics
        .preparation_tables
        .iter()
        .filter(|table| matches!(table.stage, "fixed signals" | "compiled wire callbacks"))
        .map(|table| serde_json::json!({"stage":table.stage,"table":table.table,"entries":table.entries,"estimated_bytes":table.estimated_bytes}))
        .collect::<Vec<_>>();
    let report = serde_json::json!({
        "experiment":"paired general and compiled classic electrical, callback and pending queries; neither engine eliminates dust walks", "comparison":comparison,
        "profile":if cfg!(debug_assertions) {"debug"} else {"release"},"ticks":ticks,"checked_boundaries":ticks+1,
        "protocol":"same lowered CPU/program/state, original start button once, no prewarm, alternating tick order, full state/FIFO checks outside timers",
        "compile_seconds":compile_seconds,"general_seconds":execution[0].as_secs_f64(),"compiled_seconds":execution[1].as_secs_f64(),
        "general_activation_seconds":activation[0].as_secs_f64(),"compiled_activation_seconds":activation[1].as_secs_f64(),
        "speed_ratio_general_over_compiled":execution[0].as_secs_f64()/execution[1].as_secs_f64(),
        "general_work":format!("{:?}",engines[0].execution_counts()),"compiled_work":format!("{:?}",engines[1].execution_counts()),"tables":tables,
        "compiled_callback_wires":engines[1].program.wires().iter().filter(|wire|wire.execution.callback_selection.is_some_and(|selection|selection.known==WireCallbackSelection::ALL)).count(),
        "total_wires":engines[1].program.wires().len(),
        "general_callback_index_bytes":std::mem::size_of_val(engines[0].program.turbo_callback_indices()),
        "compiled_callback_index_bytes":std::mem::size_of_val(engines[1].program.turbo_callback_indices()),
        "limits":"unit-test query instrumentation remains enabled; equivalence against the existing graph, not a new interpreter oracle"
    });
    let prefix = match comparison.as_str() {
        "queries" => "fixed-query",
        "pending" => "pending-query",
        "callbacks" => "fixed-callback",
        _ => "query-and-pending",
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/chungus-{prefix}-paired-{ticks}.json"));
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("CHUNGUS FIXED QUERIES {report}");
}
