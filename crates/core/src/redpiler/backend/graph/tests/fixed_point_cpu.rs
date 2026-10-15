//! Bounded research capture: identify the first CPU state divergence, not benchmark speed.
use super::*;
use serde_json::json;

#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

#[test]
#[ignore = "research: prepares the full CPU and compares exact and settled graph histories"]
fn cpu_fixed_point_matches_initial_cells_but_changes_resources_and_pending_order() {
    let cpu = cpus::CPUS[2];
    let mut world = cpus::load_cpu(cpu);
    world.set_block(cpu.origin + BlockPos::new(186, 2, 184), Block::Air);
    let bounds = (cpu.origin, cpu.origin + BlockPos::new(224, 41, 253));
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let (program, state) = prepared.graph.lower().unwrap();
    let mut exact = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
    let mut settled = GraphBackend::new(program, state, 0).unwrap();
    drop(world);
    let mut boundaries = Vec::new();
    for tick in 0..=20 {
        exact.set_test_recording(tick <= 8);
        settled.set_test_recording(tick <= 8);
        exact.clear_test_traces();
        settled.clear_test_traces();
        for (backend, mode) in [(&mut exact, false), (&mut settled, true)] {
            fixed_point::with_mode(mode, || {
                if tick == 0 {
                    backend.use_block(cpu.origin + cpu.start)
                } else {
                    backend.tick()
                }
            })
            .unwrap();
        }
        let differences: Vec<_> = exact
            .state
            .components
            .iter()
            .zip(&settled.state.components)
            .enumerate()
            .filter_map(|(index, (a, b))| {
                (a != b).then(|| {
                    json!({"position":exact.program.nodes()[index].position,"kind":format!("{:?}",exact.program.nodes()[index].kind),
                "exact":format!("{a:?}"),"settled":format!("{b:?}")})
                })
            })
            .collect();
        let accepted = |backend: &GraphBackend| {
            backend
                .observed_operations
                .iter()
                .filter(|op| matches!(op, GraphObservedOperation::Request { .. } | GraphObservedOperation::Accepted { .. }))
                .map(|op| format!("{op:?}"))
                .collect::<Vec<_>>()
        };
        let a = accepted(&exact);
        let b = accepted(&settled);
        let first = a.iter().zip(&b).position(|(a, b)| a != b).unwrap_or(a.len().min(b.len()));
        let window = |ops: &[String]| ops[first.saturating_sub(2)..(first + 4).min(ops.len())].to_vec();
        let piston_differences: Vec<_> = exact.state.pistons.iter().zip(&settled.state.pistons).enumerate().filter_map(|(index,(a,b))| {
            (a != b).then(|| json!({"position":exact.program.nodes()[exact.program.pistons()[index].node.index()].position,"exact":format!("{a:?}"),"settled":format!("{b:?}")}))
        }).collect();
        let requests = |backend: &GraphBackend| {
            backend
                .piston_requests
                .iter()
                .map(|request| (request.piston, request.action))
                .collect::<Vec<_>>()
        };
        let resource_differences: Vec<_> = exact
            .state
            .resources
            .iter()
            .zip(&settled.state.resources)
            .enumerate()
            .filter_map(|(index, (a, b))| (a != b).then(|| json!({"resource":index,"exact":format!("{a:?}"),"settled":format!("{b:?}" )})))
            .collect();
        let requests_equal = requests(&exact) == requests(&settled);
        let scheduled_equal = exact.scheduled_ticks() == settled.scheduled_ticks();
        let deferred_equal = format!("{:?}", exact.deferred_pistons) == format!("{:?}", settled.deferred_pistons);
        println!(
            "CPU t{tick}: {} component differences, {} piston differences; exact {:?}; settled {:?}",
            differences.len(),
            piston_differences.len(),
            exact.execution_counts(),
            settled.execution_counts()
        );
        boundaries.push(
            json!({"tick":tick,"component_difference_count":differences.len(),"components":differences.iter().take(16).collect::<Vec<_>>(),
            "piston_difference_count":piston_differences.len(),"pistons":piston_differences.iter().take(8).collect::<Vec<_>>(),
            "resource_difference_count":resource_differences.len(),"resources":resource_differences,"pending_requests_equal":requests_equal,
            "scheduled_order_equal":scheduled_equal,"deferred_order_equal":deferred_equal,
            "first_request_difference":first,"exact_requests":window(&a),"settled_requests":window(&b)}),
        );
    }
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fp-cpu-first-divergence.json");
    std::fs::write(output, serde_json::to_vec_pretty(&boundaries).unwrap()).unwrap();
    // The external-wire fix repairs the initial CPU states, not necessarily its future ordering.
    assert!(
        boundaries
            .iter()
            .all(|row| row["component_difference_count"] == 0 && row["piston_difference_count"] == 0)
    );
    assert!(
        boundaries.iter().any(|row| row["resource_difference_count"].as_u64().unwrap() != 0),
        "matching cells do not establish shared-resource equivalence"
    );
}
