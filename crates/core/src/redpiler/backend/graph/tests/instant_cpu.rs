//! Active CPU ownership and veto census; these counters are absent from production builds.
use super::*;
use serde_json::json;
use std::collections::BTreeMap;

#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

#[test]
#[ignore = "research: active BubbleSort Instant actors, proof cuts and runtime vetoes"]
fn bubblesort_instant_active_coverage_census() {
    let cpu = cpus::CPUS[2];
    let mut world = cpus::load_cpu(cpu);
    let sign = cpu.origin + BlockPos::new(186, 2, 184);
    assert!(matches!(world.get_block(sign), Block::WallSign { .. }));
    assert!(world.set_block(sign, Block::Air));
    world.flush_block_changes();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(5),
        stable_instant_inputs: true,
        budget_multiplier: 8,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        &world,
        world.get_corners(),
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
    )
    .unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let plan = graph.plans.instant.layers.as_ref().unwrap();
    let mut family = vec!["unadmitted".to_owned(); graph.pistons.len()];
    let mut status = vec!["unadmitted".to_owned(); graph.pistons.len()];
    for cell in &catalog.cells {
        for use_ in &cell.uses {
            family[use_.actor.index()] = match use_.reset {
                CellReset::Observer { .. } => "observer",
                CellReset::Payload { .. } => "payload",
                CellReset::InputRestoration { .. } => "input_restoration",
            }
            .into();
            status[use_.actor.index()] = "symbolic_only".into();
        }
    }
    let mut groups = Vec::new();
    for decline in &plan.declines {
        let actors: Vec<_> = decline
            .cells
            .iter()
            .flat_map(|&cell| catalog.cells[cell as usize].uses.iter().map(|use_| use_.actor))
            .collect();
        for actor in &actors {
            status[actor.index()] = format!("{:?}", decline.reason);
        }
        groups.push((
            actors,
            json!({"reason":format!("{:?}",decline.reason),"witness":format!("{:?}",decline.witness),
            "cells":decline.cells.len(),"corridors":decline.corridors.len()}),
        ));
    }
    let mut backend = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
    backend.set_test_recording(false);
    backend.test_recorder.memory_cell_activity = vec![[0; 2]; backend.program.pistons().len()];
    backend.test_recorder.instant_kernel_activity = vec![[0; 7]; backend.program.pistons().len()];
    backend.use_block(cpu.origin + cpu.start).unwrap();
    for _ in 0..1000 {
        backend.tick_with_world(&mut world).unwrap();
    }
    let counts = backend.execution_counts();
    let mut categories = BTreeMap::<String, [u64; 2]>::new();
    let mut outcomes = [0u64; 7];
    let mut rows = Vec::new();
    for (index, actor) in backend.program.pistons().iter().enumerate() {
        let kernel = backend.program.instant_kernel_for_actor(PistonId::new(index as u32));
        if kernel.is_some() {
            status[index] = "kernel".into();
        }
        let activity = backend.test_recorder.memory_cell_activity[index];
        let key = format!("{}:{}", family[index], status[index]);
        for (total, value) in categories.entry(key).or_default().iter_mut().zip(activity) {
            *total += value;
        }
        for (total, value) in outcomes.iter_mut().zip(backend.test_recorder.instant_kernel_activity[index]) {
            *total += value;
        }
        rows.push(
            json!({"actor":index,"position":actor.position,"family":family[index],"status":status[index],
            "samples":activity[0],"actions":activity[1],"outcomes":backend.test_recorder.instant_kernel_activity[index],
            "kernel_actors":kernel.map(|kernel|kernel.actors.len()),"kernel_wires":kernel.map(|kernel|kernel.wires.len())}),
        );
    }
    for (actors, group) in &mut groups {
        let activity = actors.iter().fold([0u64; 2], |mut sum, actor| {
            for (total, value) in sum.iter_mut().zip(backend.test_recorder.memory_cell_activity[actor.index()]) {
                *total += value;
            }
            sum
        });
        group["samples"] = json!(activity[0]);
        group["actions"] = json!(activity[1]);
        group["actors"] = json!(actors.iter().map(|actor| actor.index()).collect::<Vec<_>>());
    }
    groups.sort_by_key(|(_, group)| std::cmp::Reverse(group["actions"].as_u64().unwrap()));
    assert_eq!(categories.values().map(|value| value[0]).sum::<u64>(), counts.piston_samples);
    assert_eq!(categories.values().map(|value| value[1]).sum::<u64>(), counts.piston_actions);
    assert_eq!(outcomes[6], counts.instant_kernel_invocations);
    assert!(counts.scheduled_ticks > 0 && counts.piston_actions > 0);
    let report = json!({"ticks":1000,"schematic_sha256":cpu.sha256,"categories":categories,"outcome_totals":outcomes,
        "outcome_columns":["no_kernel","no_origin","unsupported_route","entry_state","predicate","queue","executed"],
        "counts":format!("{counts:?}"),"declined_regions":groups.into_iter().map(|(_,group)|group).collect::<Vec<_>>(),"actors":rows});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bubblesort-instant-active-census.json");
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("active Instant categories: {categories:?}; outcomes={outcomes:?}");
}
