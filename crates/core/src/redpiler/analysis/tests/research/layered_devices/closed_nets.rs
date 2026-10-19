//! Test the stronger boundary independently of the existing private/single-cable kernel family.
use super::*;
use crate::redpiler::analysis::instant::regions;
use crate::redpiler::passes::planning::PlanBuilder;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
#[ignore = "explicit whole closed-net boundary census and native contraction counterexamples"]
fn closed_instant_net_boundaries_and_native_probe() {
    for name in ["cpu_bubblesort", "fpu"] {
        let load_world = || {
            if name == "cpu_bubblesort" {
                let (world, bounds, _) = large_builds::load_cpu(cpus::CPUS[2]);
                (world, bounds)
            } else {
                load(&manifest("fpu_legal"))
            }
        };
        let (mut native, bounds) = load_world();
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(8);
        let options = CompilerOptions {
            optimization_level: Some(5),
            stable_instant_inputs: true,
            budget_multiplier: 8,
            ..Default::default()
        };
        let mut prepared = passes::rich::prepare(
            &native,
            bounds,
            native.scheduler().iter_entries().collect(),
            &monitor,
            AnalysisLimits::for_budget(8),
        )
        .unwrap();
        let mut pipeline = passes::default_graph_pipeline(&options);
        pipeline.entries.retain(|entry|!matches!(&entry.pass,passes::Pass::Build(builder) if matches!(builder.name(),"unsafe-abstract-instant-nets"|"experimental-compile-instant-wire-nodes")));
        passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
        let graph = &prepared.graph;
        let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
        let start = std::time::Instant::now();
        let census = census_closed_instant_nets(graph, catalog, &monitor).unwrap();
        let census_seconds = start.elapsed().as_secs_f64();
        let mut joinable = vec![true; catalog.cells.len() + catalog.corridors.len()];
        for flag in &mut joinable[catalog.cells.len()..] {
            *flag = false;
        }
        for net in &census.closed {
            joinable[catalog.cells.len() + net.corridor as usize] = true;
        }
        let expanded=regions::partition_with_cuts(graph,catalog,&monitor,&joinable,|kind|match kind {
            BoundaryPortKind::Power {link} => !matches!(graph.sites[graph.power_links[link as usize].from.index()].source,SiteSource::Component(node) if matches!(graph.nodes[node.index()].kind,CircuitNodeKind::Observer {..}|CircuitNodeKind::BuiltinReset {..})),
            BoundaryPortKind::Callback {occurrence} => !matches!(graph.nodes[graph.routes[occurrence.route.index()].source.index()].kind,CircuitNodeKind::Observer {..}|CircuitNodeKind::BuiltinReset {..}),
            _ => false,
        }).unwrap();
        let old = graph.plans.instant.layers.as_ref().unwrap();
        let mut declines = BTreeMap::<String, usize>::new();
        for decline in &old.declines {
            *declines.entry(format!("{:?}", decline.reason)).or_default() +=
                decline.cells.iter().map(|&cell| catalog.cells[cell as usize].uses.len()).sum::<usize>();
        }
        let mut groups:Vec<_>=expanded.regions.iter().map(|region|json!({"cells":region.cells.len(),"actors":region.cells.iter().map(|&id|catalog.cells[id as usize].uses.len()).sum::<usize>(),"wires":region.corridors.iter().map(|&id|catalog.corridors[id as usize].wires.len()).sum::<usize>(),"corridors":region.corridors.len()})).collect();
        groups.sort_by_key(|row| std::cmp::Reverse(row["actors"].as_u64().unwrap()));
        let mut report = json!({"name":name,"census_seconds":census_seconds,"closed_nets":census.closed.len(),
            "closed_multi_wire_nets":census.closed.iter().filter(|net|net.wires>1).count(),"closed_multi_wire_members":census.closed.iter().filter(|net|net.wires>1).map(|net|net.wires).sum::<usize>(),
            "declined_nets_and_wires":census.declines,"nets":census.closed.iter().map(|net|json!({"corridor":net.corridor,"wires":net.wires,"instant_readers":net.actors.iter().map(|actor|actor.index()).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "old_compiled_regions":old.regions.len(),"old_declined_actors_by_reason":declines,"expanded_structural_groups":groups,
            "structural_closure_is_not_fifo_certificate":true,"runtime_correctness":"pending"});
        let directory = root().join("target/instant-wire-nodes/closed-nets");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{name}.json"));
        std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!(
            "{name}: {} closed multi-wire nets / {} wires; largest structural group {:?}",
            report["closed_multi_wire_nets"], report["closed_multi_wire_members"], report["expanded_structural_groups"][0]
        );
        let clock_info:BTreeMap<_,_>=graph.nodes.iter().filter_map(|node|node.position.map(|pos|((pos.x,pos.y,pos.z),json!({"kind":format!("{:?}",node.kind),"output_deliveries":node.output_route.map(|route|graph.routes[route.index()].deliveries.len())})))).collect();
        let mut candidate = graph.clone();
        let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
        passes::abstract_instant_nets::AbstractInstantNets.run(&mut candidate, &context).unwrap();
        assert!(candidate.wires.iter().any(|wire| wire.execution.abstract_instant_net));
        let baseline = prepared.lower(&monitor).unwrap();
        let (program, state) = candidate.lower_with_monitor(&monitor).unwrap();
        let candidate = crate::redpiler::program_graph::LoweredCircuit {
            program,
            state,
            logical_tick: baseline.logical_tick,
            positions: baseline.positions.clone(),
            imported_ticks: baseline.imported_ticks.clone(),
        };
        let mut backend = GraphBackend::from_lowered(candidate, &monitor).unwrap();
        let mut baseline = GraphBackend::from_lowered(baseline, &monitor).unwrap();
        baseline.set_test_recording(false);
        backend.set_test_recording(false);
        let (mut base_world, _) = load_world();
        let (mut projected, _) = load_world();
        let trigger = if name == "cpu_bubblesort" {
            cpus::CPUS[2].origin + cpus::CPUS[2].start
        } else {
            local_pos(&manifest("fpu_legal")["ports"]["trigger"]) - BASE + origin(&manifest("fpu_legal"))
        };
        if name == "cpu_bubblesort" {
            cpus::click_cpu(&mut native, cpus::CPUS[2], cpus::CPUS[2].start);
        } else {
            let Block::Lever { lever } = native.get_block(trigger) else {
                panic!("missing FPU trigger")
            };
            lever_action(&mut native, trigger, !lever.powered);
        }
        baseline.use_block(trigger).unwrap();
        backend.use_block(trigger).unwrap();
        let mut matched = 0;
        let mut baseline_seconds = 0.0;
        let mut abstract_seconds = 0.0;
        let mut ordered_prefix = 0;
        let mut saw_order_mismatch = false;
        for tick in 0..if name == "cpu_bubblesort" { 100 } else { 128 } {
            if name == "fpu" && tick == 64 {
                let Block::Lever { lever } = native.get_block(trigger) else {
                    unreachable!()
                };
                lever_action(&mut native, trigger, !lever.powered);
                baseline.use_block(trigger).unwrap();
                backend.use_block(trigger).unwrap();
            }
            native.tick_interpreted();
            let start = std::time::Instant::now();
            baseline.tick_with_world(&mut base_world).unwrap();
            baseline_seconds += start.elapsed().as_secs_f64();
            let start = std::time::Instant::now();
            backend.tick_with_world(&mut projected).unwrap();
            abstract_seconds += start.elapsed().as_secs_f64();
            project(&baseline, &mut base_world);
            project(&backend, &mut projected);
            assert_future_work(&native, &base_world, name);
            pc_counter_contract::compare_checkpoint(&native, &base_world, bounds, "unchanged baseline");
            if !saw_order_mismatch {
                let result = catch_unwind(AssertUnwindSafe(|| assert_future_work(&native, &projected, name)));
                if let Err(error) = result {
                    saw_order_mismatch = true;
                    report["first_future_mismatch_tick"] = json!(tick + 1);
                    report["future_counterexample"] = json!(panic_message(error));
                    let expected: Vec<_> = native.scheduler().iter_entries().collect();
                    let actual: Vec<_> = projected.scheduler().iter_entries().collect();
                    report["first_clock_order_difference"]=json!(expected.iter().zip(&actual).find(|(left,right)|left!=right).map(|(left,right)|json!({
                        "native":format!("{left:?}"),"abstract":format!("{right:?}"),
                        "native_node":clock_info.get(&(left.pos.x,left.pos.y,left.pos.z)),"abstract_node":clock_info.get(&(right.pos.x,right.pos.y,right.pos.z)),
                    })));
                } else {
                    ordered_prefix += 1;
                }
            }
            let result = catch_unwind(AssertUnwindSafe(|| compare_cells(&native, &projected, bounds)));
            if let Err(error) = result {
                report["runtime_correctness"] = json!("observable_state_counterexample");
                report["first_state_mismatch_tick"] = json!(tick + 1);
                report["state_counterexample"] = json!(panic_message(error));
                break;
            }
            matched += 1;
        }
        if report["runtime_correctness"] == "pending" {
            report["runtime_correctness"] = json!(if saw_order_mismatch {
                "bounded_state_match_with_future_reordering"
            } else {
                "bounded_native_pass"
            });
        }
        report["native_state_ticks_matched"] = json!(matched);
        report["strict_future_prefix_matched"] = json!(ordered_prefix);
        report["diagnostic_tick_seconds"] = json!({"baseline":baseline_seconds,"abstract":abstract_seconds,"clean_paired_benchmark":false});
        report["work_counts"] = json!(format!("{:?}", backend.execution_counts()));
        std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!("{name}: closure contraction {}, matched {matched} ticks", report["runtime_correctness"]);
    }
}

fn panic_message(error: Box<dyn std::any::Any + Send>) -> String {
    error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| error.downcast_ref::<&str>().map(|text| text.to_string()))
        .unwrap_or_else(|| "non-string mismatch".into())
}

fn compare_cells(native: &PlotWorld, projected: &PlotWorld, bounds: (BlockPos, BlockPos)) {
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let position = BlockPos::new(x, y, z);
                assert_eq!(
                    native.get_block(position),
                    projected.get_block(position),
                    "observable block at {position:?}"
                );
                assert_eq!(
                    serde_json::to_value(native.get_block_entity(position)).unwrap(),
                    serde_json::to_value(projected.get_block_entity(position)).unwrap(),
                    "observable entity at {position:?}"
                );
            }
        }
    }
}
