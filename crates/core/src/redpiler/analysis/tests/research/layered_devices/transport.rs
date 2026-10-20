//! Complete-state transport validation and isolated before/after inference timing.
use super::*;
use std::time::Instant;

fn prepare_cpu_reference(
    world: &PlotWorld,
    bounds: (BlockPos, BlockPos),
    channels_only: bool,
    stable_inputs: bool,
) -> (LoweredCircuit, CircuitProgram, Vec<(PistonId, BlockPos)>) {
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        stable_instant_inputs: stable_inputs,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        world,
        bounds,
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
    let plan = &prepared.graph.plans.instant_transport;
    println!(
        "BubbleSort transition eligibility: {:?}; net15={}, net76={}",
        prepared.graph.plans.instant_transition_programs.stats,
        prepared.graph.plans.instant_transition_programs.nets.get(15).is_some_and(Option::is_some),
        prepared.graph.plans.instant_transition_programs.nets.get(76).is_some_and(Option::is_some)
    );
    let covered = plan.nets.iter().map(|net| net.wires.len()).sum::<usize>();
    println!(
        "BubbleSort dense transport: {} programs, {covered}/{} wires",
        plan.nets.len(),
        prepared.graph.wires.len()
    );
    assert!(covered > 0);
    let memories = memory_only::memory_probes(&prepared.graph, "cpu_bubblesort");
    assert_eq!(memories.len(), 768);
    if stable_inputs {
        let mut baseline = prepared.graph.clone();
        baseline.plans.guard_predicates = Default::default();
        let (reference, _) = baseline.lower_with_monitor(&monitor).unwrap();
        let lowered = prepared.lower(&monitor).unwrap();
        return (lowered, reference, memories);
    }
    let mut baseline = prepared.graph.clone();
    if !channels_only {
        baseline.plans.instant_transport = Default::default();
        baseline.plans.instant_drives = Default::default();
        baseline.plans.instant_value_regions = Default::default();
        baseline.plans.instant_transition_programs = Default::default();
    }
    baseline.plans.signal_channels = Default::default();
    baseline.plans.guard_predicates = Default::default();
    let (baseline, _) = baseline.lower_with_monitor(&monitor).unwrap();
    (prepared.lower(&monitor).unwrap(), baseline, memories)
}

fn instance(lowered: &LoweredCircuit, program: CircuitProgram) -> GraphBackend {
    let mut backend = GraphBackend::from_lowered(
        LoweredCircuit {
            program,
            state: lowered.state.clone(),
            logical_tick: lowered.logical_tick,
            positions: lowered.positions.clone(),
            imported_ticks: lowered.imported_ticks.clone(),
        },
        &TaskMonitor::default(),
    )
    .unwrap();
    backend.set_test_recording(false);
    backend
}

fn sorted_ram(backend: &GraphBackend) {
    let cpu = cpus::CPUS[2];
    let mut projected = empty();
    project(backend, &mut projected);
    for row in 0..16 {
        let word = (0..12).fold(0u16, |word, bit| {
            let Block::Piston { piston } = projected.get_block(cpu.origin + BlockPos::new(175 + 4 * bit, 37, 62 - 4 * row)) else {
                panic!("RAM unavailable")
            };
            word | (u16::from(!piston.extended) << bit)
        });
        assert_eq!(word, row as u16, "complete sort RAM row {row}");
    }
}

#[test]
#[ignore = "one full BubbleSort inference for test-only callback receiver attribution and per-net transport work"]
fn bubblesort_callback_receiver_census() {
    let cpu = cpus::CPUS[2];
    let (world, bounds, _) = large_builds::load_cpu(cpu);
    let (lowered, _, _) = prepare_cpu_reference(&world, bounds, false, true);
    let mut candidate = instance(&lowered, lowered.program.clone());
    candidate.enable_callback_census();
    let mut host = empty();
    host.disable_command_output_limits_for_replay();
    candidate.use_block(cpu.origin + cpu.start).unwrap();
    for tick in 1..=10036 {
        candidate.tick_with_world(&mut host).unwrap();
        if tick % 2048 == 0 {
            println!("callback census: {tick}/10036 ticks");
        }
    }
    sorted_ram(&candidate);

    let counts = candidate.execution_counts();
    let census = candidate.callback_census();
    let accepted = census.accepted.iter().flatten().sum::<u64>();
    assert_eq!(
        accepted, counts.callbacks,
        "test-only attribution must cover every accepted callback exactly once"
    );
    assert_eq!(counts.callbacks, 174_026_524, "match the frozen full-sort semantic work count");
    assert_eq!(counts.piston_samples, 33_492_510);
    assert_eq!(counts.piston_actions, 3_295_083);

    let accepted_by_kind = serde_json::json!({
        "update": {
            "wire": census.accepted[0][0], "piston": census.accepted[0][1],
            "observer": census.accepted[0][2], "electrical_other": census.accepted[0][3],
            "contact": census.accepted[0][4],
        },
        "shape": {
            "wire": census.accepted[1][0], "piston": census.accepted[1][1],
            "observer": census.accepted[1][2], "electrical_other": census.accepted[1][3],
            "contact": census.accepted[1][4],
        },
    });
    let metadata = candidate.transport_census_metadata();
    let mut hot_nets = census
        .transport_by_net
        .iter()
        .enumerate()
        .filter_map(|(index, work)| {
            (work.invocations > 0).then(|| {
                let mut row = metadata[index].clone();
                row["net"] = serde_json::json!(index);
                row["invocations"] = serde_json::json!(work.invocations);
                row["wire_visits"] = serde_json::json!(work.wire_visits);
                row
            })
        })
        .collect::<Vec<_>>();
    hot_nets.sort_by_key(|row| std::cmp::Reverse(row["wire_visits"].as_u64().unwrap_or(0)));
    hot_nets.truncate(20);
    let report = serde_json::json!({
        "build": "cpu_bubblesort",
        "source_sha256": cpu.sha256,
        "ticks": 10036,
        "sorted_ram": true,
        "original_world_used_for_graph_inference": false,
        "accepted_callbacks": accepted,
        "frozen_expected_callbacks": 174026524u64,
        "accepted_by_kind_and_receiver": accepted_by_kind,
        "piston_receiver_candidates_before_guards": census.piston_candidates,
        "piston_candidates_base_pending_before_guards": census.piston_candidates_base_pending,
        "accepted_piston_deliveries_base_pending": census.accepted_piston_base_pending,
        "piston_samples": counts.piston_samples,
        "piston_actions": counts.piston_actions,
        "transport_invocations": census.transport_by_net.iter().map(|item| item.invocations).sum::<u64>(),
        "transport_wire_visits": census.transport_by_net.iter().map(|item| item.wire_visits).sum::<u64>(),
        "top_transport_nets_by_wire_visits": hot_nets,
        "queued_same_action_candidates": "not instrumented; requires scanning a potentially large FIFO",
    });
    let path = root().join("target/guarded-transport/callback-census.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("BubbleSort callback census saved to {} ({accepted} callbacks)", path.display());
}

#[test]
#[ignore = "one full BubbleSort inference for test-only hot value-region and boundary attribution"]
fn bubblesort_value_region_execution_census() {
    let cpu = cpus::CPUS[2];
    let (world, bounds, _) = large_builds::load_cpu(cpu);
    let (lowered, _, _) = prepare_cpu_reference(&world, bounds, false, true);
    let mut candidate = instance(&lowered, lowered.program.clone());
    candidate.enable_value_region_census();
    let mut host = empty();
    host.disable_command_output_limits_for_replay();
    candidate.use_block(cpu.origin + cpu.start).unwrap();
    for tick in 1..=10_036 {
        candidate.tick_with_world(&mut host).unwrap();
        if tick % 2_048 == 0 {
            println!("value-region census: {tick}/10036 ticks");
        }
    }
    sorted_ram(&candidate);

    let counts = candidate.execution_counts();
    assert_eq!(counts.callbacks, 174_026_524, "match the frozen full-sort semantic work count");
    assert_eq!(counts.piston_samples, 33_492_510);
    assert_eq!(counts.piston_actions, 3_295_083);
    assert_eq!(counts.instant_transport_wire_visits, 95_393_263);
    let census = candidate.value_region_census();
    let metadata = candidate.transport_census_metadata();
    let mut rows = Vec::new();
    let mut simple_visits = 0u64;
    let mut simple_inert = 0u64;
    let mut simple_writes = 0u64;
    let mut simple_boundary_closed_visits = 0u64;
    let mut simple_boundary_closed_inert = 0u64;
    let mut simple_boundary_closed_writes = 0u64;
    let drive_program = candidate.value_region_census_drive_program();
    for (net_index, region_rows) in census.by_net.iter().enumerate() {
        let Some((net, values)) = candidate.value_region_census_program_net(net_index as u32) else {
            continue;
        };
        // A non-wire site is a shared capture dependency when wires in more than
        // one value region reference it. Keep the site->region index local to this
        // net; these are structural references, not chronological deliveries.
        let mut site_regions = vec![std::collections::BTreeSet::<usize>::new(); net.sites.len()];
        for (wire_index, item) in net.wires.iter().enumerate() {
            let region_id = values.wire_region[wire_index];
            if region_id == u32::MAX {
                continue;
            }
            for &site in &item.neighbors {
                if site != u32::MAX && net.site_wire[site as usize] == u32::MAX {
                    site_regions[site as usize].insert(region_id as usize);
                }
            }
        }
        for (region_index, work) in region_rows.iter().enumerate() {
            let visits = work.dense_visits + work.inert_visits;
            if visits == 0 {
                continue;
            }
            let region = &values.regions[region_index];
            let member_wires = &values.region_wires[region.wires.start as usize..region.wires.end as usize];
            let wires = member_wires
                .iter()
                .map(|&wire| {
                    let item = &net.wires[wire as usize];
                    serde_json::json!({
                        "local_wire": wire,
                        "node": item.node.index(),
                        "position": candidate.value_region_census_node_position(item.node),
                    })
                })
                .collect::<Vec<_>>();
            let drives = values.drive_groups[region.drive_groups.start as usize..region.drive_groups.end as usize]
                .iter()
                .map(|group| {
                    let query = &drive_program.queries[group.query as usize];
                    let terms = drive_program.terms[query.terms.start as usize..query.terms.end as usize]
                        .iter()
                        .map(|term| {
                            serde_json::json!({
                                "source": format!("{:?}", term.source),
                                "condition": format!("{:?}", drive_program.conditions[term.condition as usize]),
                                "attenuation": term.attenuation,
                            })
                        })
                        .collect::<Vec<_>>();
                    serde_json::json!({"query":group.query,"constant":query.constant,"terms":terms})
                })
                .collect::<Vec<_>>();
            let selectors = values.conditional_arcs[region.conditional_arcs.start as usize..region.conditional_arcs.end as usize]
                .iter()
                .map(|arc| {
                    let selector = values.selectors[arc.selector as usize];
                    serde_json::json!({
                        "source_region_wire": arc.source,
                        "selector": arc.selector,
                        "horizontal": candidate.value_region_census_site_position(selector.horizontal_site),
                        "ascent_up": selector.ascent_up_site.and_then(|site| candidate.value_region_census_site_position(site)),
                    })
                })
                .collect::<Vec<_>>();

            let mut receiver_counts = std::collections::BTreeMap::<String, u64>::new();
            let mut ordered_boundary_sample = Vec::new();
            let mut foreign_wire_slots = 0usize;
            let mut shared_nonwire_sites = std::collections::BTreeSet::<u32>::new();
            for &wire in member_wires {
                let item = &net.wires[wire as usize];
                let route = candidate.value_region_census_route(item.route);
                for (slot, &local_site) in item.neighbors.iter().enumerate() {
                    if local_site == u32::MAX {
                        continue;
                    }
                    let target_wire = net.site_wire[local_site as usize];
                    if target_wire != u32::MAX {
                        let target_region = values.wire_region[target_wire as usize];
                        if target_region != u32::MAX && target_region as usize != region_index {
                            foreign_wire_slots += 1;
                        }
                        continue;
                    }
                    if site_regions[local_site as usize].iter().any(|&other| other != region_index) {
                        shared_nonwire_sites.insert(local_site);
                    }
                    let site = net.sites[local_site as usize];
                    for (sequence, delivery) in route
                        .deliveries
                        .iter()
                        .enumerate()
                        .filter(|(_, delivery)| delivery.target == site && delivery.receiver.is_some())
                    {
                        *receiver_counts.entry(format!("{:?}", delivery.receiver.unwrap())).or_default() += 1;
                        if ordered_boundary_sample.len() < 128 {
                            ordered_boundary_sample.push(serde_json::json!({
                                "source_wire": wire,
                                "slot": slot,
                                "route": item.route.index(),
                                "sequence": sequence,
                                "delivery_kind": format!("{:?}", delivery.kind),
                                "receiver": format!("{:?}", delivery.receiver.unwrap()),
                            }));
                        }
                    }
                }
            }
            let one_drive_unconditional = drives.len() == 1 && selectors.is_empty();
            if one_drive_unconditional {
                simple_visits += visits;
                simple_inert += work.inert_visits;
                simple_writes += work.changed_wire_writes;
                if foreign_wire_slots == 0 && shared_nonwire_sites.is_empty() {
                    simple_boundary_closed_visits += visits;
                    simple_boundary_closed_inert += work.inert_visits;
                    simple_boundary_closed_writes += work.changed_wire_writes;
                }
            }
            let mut row = serde_json::json!({
                "net": net_index,
                "region": region_index,
                "component": region.component,
                "visits": visits,
                "dense_visits": work.dense_visits,
                "inert_visits": work.inert_visits,
                "changed_wire_writes": work.changed_wire_writes,
                "wire_count": member_wires.len(),
                "drive_group_count": drives.len(),
                "conditional_arc_count": selectors.len(),
                "foreign_mapped_wire_neighbor_slots": foreign_wire_slots,
                "distinct_nonwire_capture_sites_shared_with_other_regions": shared_nonwire_sites.len(),
                "wires": wires,
                "drives": drives,
                "conditional_selectors": selectors,
                "ordered_boundary_receiver_refs": receiver_counts,
                "ordered_boundary_ref_sample": ordered_boundary_sample,
                "ordered_boundary_ref_sample_semantics": "structural references from canonical slots and route rows; not an observed chronological delivery trace",
            });
            row["net_metadata"] = metadata.get(net_index).cloned().unwrap_or(serde_json::Value::Null);
            rows.push(row);
        }
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row["visits"].as_u64().unwrap_or(0)));
    rows.truncate(100);
    let total_visits = census.by_net.iter().flatten().map(|row| row.dense_visits + row.inert_visits).sum::<u64>();
    let total_inert = census.by_net.iter().flatten().map(|row| row.inert_visits).sum::<u64>();
    let total_writes = census.by_net.iter().flatten().map(|row| row.changed_wire_writes).sum::<u64>();
    assert_eq!(
        total_visits, counts.instant_transport_wire_visits,
        "every transport visit must be attributed to a value region"
    );
    assert_eq!(
        total_inert, counts.instant_value_region_inert_visits,
        "inert visits must match the runtime total"
    );
    let report = serde_json::json!({
        "build": "cpu_bubblesort",
        "source_sha256": cpu.sha256,
        "ticks": 10036,
        "sorted_ram": true,
        "callbacks": counts.callbacks,
        "piston_samples": counts.piston_samples,
        "piston_actions": counts.piston_actions,
        "transport_wire_visits": counts.instant_transport_wire_visits,
        "value_region_visits": total_visits,
        "value_region_inert_visits": total_inert,
        "changed_wire_writes": total_writes,
        "all_inert_transport_invocations": census.all_inert_invocations,
        "visits_in_all_inert_transport_invocations": census.all_inert_visits,
        "unattributed_transport_visits": counts.instant_transport_wire_visits - total_visits,
        "one_drive_zero_conditional_regions": {
            "visits": simple_visits,
            "inert_visits": simple_inert,
            "changed_wire_writes": simple_writes,
            "surviving_zero_foreign_wire_slots_and_zero_shared_nonwire_capture_sites": {
                "visits": simple_boundary_closed_visits,
                "inert_visits": simple_boundary_closed_inert,
                "changed_wire_writes": simple_boundary_closed_writes,
            },
        },
        "top_regions_by_visits": rows,
    });
    let path = root().join("target/value-regions-cached/region-census.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("BubbleSort value-region census saved to {} ({total_visits} visits)", path.display());
}

#[test]
#[ignore = "active BubbleSort smoke check: every component, piston, resource and pending item"]
fn bubblesort_dense_transport_first_128_ticks_preserve_exact_state() {
    validate_cpu(128, false);
}

#[test]
#[ignore = "full native BubbleSort: every graph state/future boundary and all 768 native RAM bits"]
fn bubblesort_dense_transport_complete_sort_matches_native_and_future() {
    validate_cpu(10036, false);
}

#[test]
#[ignore = "full native BubbleSort and exact per-tick state/future ablation of the compiled signal channel pass"]
fn bubblesort_signal_channels_complete_sort_matches_native_and_future() {
    validate_cpu(10036, true);
}

fn validate_cpu(ticks: usize, channels_only: bool) {
    let cpu = cpus::CPUS[2];
    let (mut native, bounds, _) = large_builds::load_cpu(cpu);
    native.disable_command_output_limits_for_replay();
    let (lowered, reference, memories) = prepare_cpu_reference(&native, bounds, channels_only, false);
    let mut baseline = instance(&lowered, reference);
    let mut candidate = instance(&lowered, lowered.program.clone());
    let mut baseline_host = empty();
    let mut candidate_host = empty();
    baseline_host.disable_command_output_limits_for_replay();
    candidate_host.disable_command_output_limits_for_replay();
    cpus::click_cpu(&mut native, cpu, cpu.start);
    baseline.use_block(cpu.origin + cpu.start).unwrap();
    candidate.use_block(cpu.origin + cpu.start).unwrap();
    candidate.assert_same_execution_state(&baseline);
    for tick in 1..=ticks {
        baseline.tick_with_world(&mut baseline_host).unwrap();
        candidate.tick_with_world(&mut candidate_host).unwrap();
        candidate.assert_same_execution_state(&baseline);
        native.tick_interpreted();
        for &(actor, position) in &memories {
            assert_eq!(
                memory_only::graph_storage_bit(&candidate, actor),
                memory_only::native_storage_bit(&native, position),
                "RAM {position:?}, tick {tick}"
            );
        }
        if tick % 1024 == 0 {
            println!("dense transport: {tick} exact CPU boundaries");
        }
    }
    assert!(candidate.execution_counts().instant_transport_invocations > 0);
    assert_eq!(
        baseline_host.command_output().collect::<Vec<_>>(),
        candidate_host.command_output().collect::<Vec<_>>()
    );
    if ticks == 10036 {
        sorted_ram(&candidate);
        let (mut resumed, _, _) = large_builds::load_cpu(cpu);
        resumed.disable_command_output_limits_for_replay();
        candidate.reset(&mut resumed);
        for tick in 1..=16 {
            native.tick_interpreted();
            resumed.tick_interpreted();
            for &(_, position) in &memories {
                assert_eq!(
                    memory_only::native_storage_bit(&resumed, position),
                    memory_only::native_storage_bit(&native, position),
                    "dense transport native continuation RAM {position:?}, tick {tick}"
                );
            }
        }
    }
    println!(
        "dense transport baseline {:?}\ncandidate {:?}",
        baseline.execution_counts(),
        candidate.execution_counts()
    );
}

#[test]
#[ignore = "five alternating full-sort O5 versus dense transport inference pairs"]
fn bubblesort_dense_transport_paired_inference() {
    paired_inference(false);
}

#[test]
#[ignore = "five alternating complete-sort inference pairs isolating the compiled signal channel pass"]
fn bubblesort_signal_channels_paired_inference() {
    paired_inference(true);
}

fn paired_inference(channels_only: bool) {
    let cpu = cpus::CPUS[2];
    let (world, bounds, _) = large_builds::load_cpu(cpu);
    let (lowered, reference, _) = prepare_cpu_reference(&world, bounds, channels_only, false);
    let mut samples = Vec::new();
    let directory = root().join(if channels_only {
        "target/signal-channels"
    } else {
        "target/instant-transport"
    });
    std::fs::create_dir_all(&directory).unwrap();
    for pair in 0..5 {
        let mut runs = Vec::new();
        for enabled in if pair % 2 == 0 { [false, true] } else { [true, false] } {
            let mut backend = instance(&lowered, if enabled { lowered.program.clone() } else { reference.clone() });
            let mut host = empty();
            host.disable_command_output_limits_for_replay();
            let start = Instant::now();
            backend.use_block(cpu.origin + cpu.start).unwrap();
            for _ in 0..10036 {
                backend.tick_with_world(&mut host).unwrap();
            }
            let seconds = start.elapsed().as_secs_f64();
            runs.push((enabled, seconds, backend, host));
        }
        runs.sort_by_key(|run| run.0);
        runs[1].2.assert_same_execution_state(&runs[0].2);
        assert_eq!(
            runs[0].3.command_output().collect::<Vec<_>>(),
            runs[1].3.command_output().collect::<Vec<_>>()
        );
        sorted_ram(&runs[1].2);
        let row = json!({"pair":pair+1,"ticks":10036,"baseline_seconds":runs[0].1,"candidate_seconds":runs[1].1,
            "baseline_tps":10036.0/runs[0].1,"candidate_tps":10036.0/runs[1].1,
            "baseline_counts":format!("{:?}",runs[0].2.execution_counts()),"candidate_counts":format!("{:?}",runs[1].2.execution_counts())});
        println!("dense transport pair {row}");
        samples.push(row);
        std::fs::write(
            directory.join("bubblesort-pairs.json"),
            serde_json::to_vec_pretty(&json!({
                "build":"CPU_BUBBLESORT","source_hash":cpu.sha256,"profile":if cfg!(debug_assertions) {"debug"} else {"release"},
                "baseline":if channels_only { "O5 with compiled signal channels removed" } else { "O5 with dense transport, typed drives and compiled signal channels removed" },
                "candidate":"O5 with all available passes",
                "ticks_per_run":10036,"exact_state_future_and_final_sort":true,"original_world_used_for_graph_inference":false,"samples":samples
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
