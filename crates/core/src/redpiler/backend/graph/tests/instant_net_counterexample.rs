//! Minimize the CPU counterexample by disabling whole experimental nets, preserving the graph.
use super::super::instant_cpu_tests::cpus;
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::passes::planning::PlanBuilder;
use rustc_hash::FxHashSet;
use serde_json::json;

const TICKS: usize = 26;

fn load_cpu() -> PlotWorld {
    let cpu = cpus::CPUS[2];
    let mut world = cpus::load_cpu(cpu);
    assert!(world.set_block(cpu.origin + BlockPos::new(186, 2, 184), Block::Air));
    world.flush_block_changes();
    world
}

fn copy_lowered(source: &LoweredCircuit) -> LoweredCircuit {
    LoweredCircuit {
        program: source.program.clone(),
        state: source.state.clone(),
        logical_tick: source.logical_tick,
        positions: source.positions.clone(),
        imported_ticks: source.imported_ticks.clone(),
    }
}

fn run(source: &LoweredCircuit, keep: &FxHashSet<NodeId>, capture: Option<usize>) -> (Vec<CircuitState>, Vec<String>, GraphExecutionCounts) {
    let monitor = TaskMonitor::default();
    let mut lowered = copy_lowered(source);
    lowered.program.retain_abstract_instant_wires(keep);
    let mut backend = GraphBackend::from_lowered(lowered, &monitor).unwrap();
    backend.set_test_recording(false);
    let mut world = load_cpu();
    backend.use_block(cpus::CPUS[2].origin + cpus::CPUS[2].start).unwrap();
    let mut states = Vec::new();
    let mut operations = Vec::new();
    for tick in 0..TICKS {
        if capture == Some(tick) {
            backend.set_test_recording(true);
        }
        backend.tick_with_world(&mut world).unwrap();
        states.push(backend.state.clone());
        if capture == Some(tick) {
            operations = backend
                .observed_operations
                .iter()
                .filter(|row| matches!(row, GraphObservedOperation::Accepted { .. }))
                .map(|row| format!("{row:?}"))
                .collect();
            backend.set_test_recording(false);
        }
    }
    (states, operations, backend.execution_counts())
}

#[test]
#[ignore = "explicit delta-debugging of closed-net CPU contraction; leaves source fixture unchanged"]
fn minimize_closed_instant_net_cpu_counterexample() {
    let world = load_cpu();
    let cpu = cpus::CPUS[2];
    let clip = load_schematic(std::io::Cursor::new(std::fs::read(root().join("test_data").join(cpu.schematic)).unwrap())).unwrap();
    let bounds = (
        cpu.origin,
        cpu.origin + BlockPos::new(clip.size_x as i32 - 1, clip.size_y as i32 - 1, clip.size_z as i32 - 1),
    );
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(5),
        stable_instant_inputs: true,
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        &world,
        bounds,
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    let mut pipeline = passes::default_graph_pipeline(&options);
    pipeline.entries.retain(|entry|!matches!(&entry.pass,Pass::Build(builder) if matches!(builder.name(),"unsafe-abstract-instant-nets"|"experimental-compile-instant-wire-nodes")));
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    let catalog = prepared.graph.plans.instant.boundaries.as_ref().unwrap().clone();
    let census = census_closed_instant_nets(&prepared.graph, &catalog, &monitor).unwrap();
    let mut candidate = prepared.graph.clone();
    let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
    passes::abstract_instant_nets::AbstractInstantNets.run(&mut candidate, &context).unwrap();
    let draft = candidate.clone();
    let baseline = prepared.lower(&monitor).unwrap();
    let (program, state) = candidate.lower_with_monitor(&monitor).unwrap();
    let candidate = LoweredCircuit {
        program,
        state,
        logical_tick: baseline.logical_tick,
        positions: baseline.positions.clone(),
        imported_ticks: baseline.imported_ticks.clone(),
    };
    let empty = FxHashSet::default();
    let (expected, _, _) = run(&baseline, &empty, None);
    let (general, _, _) = run(&candidate, &empty, None);
    assert_eq!(general, expected, "disabling all abstract nets must restore the original executor");
    let keep = |nets: &[u32]| {
        nets.iter()
            .flat_map(|&id| catalog.corridors[id as usize].wires.iter().copied())
            .collect::<FxHashSet<_>>()
    };
    let fails = |nets: &[u32]| {
        let (actual, _, _) = run(&candidate, &keep(nets), None);
        actual.iter().zip(&expected).position(|(actual, expected)| actual != expected)
    };
    let mut nets: Vec<_> = census.closed.iter().filter(|net| net.wires > 1).map(|net| net.corridor).collect();
    let first = fails(&nets).expect("the broad contraction must reproduce the known counterexample");
    let mut probes = Vec::new();
    let mut interactions = false;
    while nets.len() > 1 {
        let middle = nets.len() / 2;
        let (left, right) = nets.split_at(middle);
        if let Some(tick) = fails(left) {
            probes.push(json!({"nets":left.len(),"tick":tick+1}));
            nets = left.to_vec();
        } else if let Some(tick) = fails(right) {
            probes.push(json!({"nets":right.len(),"tick":tick+1}));
            nets = right.to_vec();
        } else {
            interactions = true;
            break;
        }
        println!("counterexample: {} nets remain", nets.len());
    }
    let tick = fails(&nets).unwrap();
    let (_, baseline_operations, base_counts) = run(&baseline, &empty, Some(tick));
    let (actual, abstract_operations, abstract_counts) = run(&candidate, &keep(&nets), Some(tick));
    let mut differences = Vec::new();
    for (index, (left, right)) in expected[tick].components.iter().zip(&actual[tick].components).enumerate() {
        if left != right {
            differences.push(json!({"node":index,"position":draft.nodes[index].position,"kind":format!("{:?}",draft.nodes[index].kind),"baseline":format!("{left:?}"),"abstract":format!("{right:?}")}));
        }
    }
    let piston_differences:Vec<_>=expected[tick].pistons.iter().zip(&actual[tick].pistons).enumerate().filter(|(_, (left,right))|left!=right).map(|(id,(left,right))|json!({"actor":id,"position":draft.pistons[id].position,"baseline":format!("{left:?}"),"abstract":format!("{right:?}")})).collect();
    let resource_differences: Vec<_> = expected[tick]
        .resources
        .iter()
        .zip(&actual[tick].resources)
        .enumerate()
        .filter(|(_, (left, right))| left != right)
        .map(|(id, (left, right))| json!({"resource":id,"baseline":format!("{left:?}"),"abstract":format!("{right:?}")}))
        .collect();
    let readers: FxHashSet<_> = census
        .closed
        .iter()
        .filter(|net| nets.contains(&net.corridor))
        .flat_map(|net| net.actors.iter().copied())
        .collect();
    let cells:Vec<_>=catalog.cells.iter().filter(|cell|cell.uses.iter().any(|use_|readers.contains(&use_.actor))).map(|cell|json!({
        "uses":cell.uses.iter().map(|use_|json!({"actor":use_.actor.index(),"position":draft.pistons[use_.actor.index()].position,"reset":format!("{:?}",use_.reset)})).collect::<Vec<_>>(),
        "resources":cell.resources.iter().map(|pin|json!({"resource":pin.resource.index(),"owners":pin.owners.iter().map(|actor|actor.index()).collect::<Vec<_>>()})).collect::<Vec<_>>(),
    })).collect();
    let first_operation = baseline_operations
        .iter()
        .zip(&abstract_operations)
        .position(|(left, right)| left != right);
    let window = first_operation
        .unwrap_or(baseline_operations.len().min(abstract_operations.len()))
        .saturating_sub(3);
    let report = json!({"original_nets":census.closed.iter().filter(|net|net.wires>1).count(),"original_first_hidden_state_difference_tick":first+1,
        "retained_nets":nets,"interaction_limited":interactions,"probes":probes,"first_state_difference_tick":tick+1,"differences":differences,
        "piston_differences":piston_differences,"resource_differences":resource_differences,"cell_contracts":cells,"first_operation_difference":first_operation,"baseline_operations":baseline_operations.iter().skip(window).take(24).collect::<Vec<_>>(),
        "abstract_operations":abstract_operations.iter().skip(window).take(24).collect::<Vec<_>>(),
        "net_wires":nets.iter().map(|&id|json!({"corridor":id,"wires":catalog.corridors[id as usize].wires.iter().map(|node|json!({"node":node.index(),"position":draft.nodes[node.index()].position})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "baseline_counts":format!("{base_counts:?}"),"abstract_counts":format!("{abstract_counts:?}"),"runtime_equivalence":false});
    let dir = root().join("target/instant-wire-nodes/closed-nets");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cpu-minimal-counterexample.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("minimized to {} nets; first state difference tick {}", nets.len(), tick + 1);
}
