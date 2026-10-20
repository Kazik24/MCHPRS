//! BubbleSort source-expression and strict shared-binary ingress census.
use super::*;

#[test]
#[ignore = "prepares the full BubbleSort graph and reports exact-u8 drive coverage and strict rank-one channel candidates"]
fn bubblesort_instant_drive_source_census() {
    let (world, bounds, _) = large_builds::load_cpu(cpus::CPUS[2]);
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(5),
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
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let stats = &prepared.graph.plans.instant_drives.stats;
    println!(
        "BubbleSort drive census: {} / {} wire drives compiled, {} fallback; {} / {} strict shared-binary wires; guarded-15 nets {}, shared predicate {}, multiple predicates {}, other shapes {}; expression work {}; strict-net declines {:?}",
        stats.compiled,
        stats.wires,
        stats.fallback,
        stats.shared_binary_wires,
        prepared
            .graph
            .plans
            .instant_transport
            .nets
            .iter()
            .map(|net| net.wires.len())
            .sum::<usize>(),
        stats.all_guarded_15_nets,
        stats.shared_predicate_nets,
        stats.multiple_predicate_nets,
        stats.other_drive_shape_nets,
        stats.work,
        stats.channel_declines
    );
    println!(
        "Complete source-shape census: {} all-guarded-15 nets, {} with one shared predicate, {} with multiple predicates, {} other drive shapes",
        stats.all_guarded_15_nets, stats.shared_predicate_nets, stats.multiple_predicate_nets, stats.other_drive_shape_nets,
    );
    let channels = &prepared.graph.plans.signal_channels.stats;
    println!(
        "Installed signal channels: {} nets, {} wires, {} transitions, {} operations; declines {:?}",
        channels.nets, channels.wires, channels.transitions, channels.operations, channels.declines
    );
    assert!(stats.compiled > 0, "the expression compiler must admit a nontrivial exact subset");
    let program = &prepared.graph.plans.instant_drives;
    let representative_nets = prepared
        .graph
        .plans
        .instant_transport
        .nets
        .iter()
        .take(8)
        .map(|net| {
            let wires = net
                .wires
                .iter()
                .map(|transport_wire| {
                    let wire_index = match &prepared.graph.nodes[transport_wire.node.index()].kind {
                        CircuitNodeKind::Wire(index) => *index,
                        _ => unreachable!("transport wires must map to wire nodes"),
                    };
                    let query = program.by_wire[wire_index].map(|index| &program.queries[index as usize]);
                    let terms = query
                        .map(|query| {
                            program.terms[query.terms.start as usize..query.terms.end as usize]
                                .iter()
                                .map(|term| {
                                    serde_json::json!({
                                        "source": format!("{:?}", term.source),
                                    "condition_id": term.condition,
                                    "condition": expand_condition(program, term.condition),
                                        "attenuation": term.attenuation,
                                    })
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    serde_json::json!({
                        "node": transport_wire.node.index(),
                        "wire_index": wire_index,
                        "constant": query.map(|query| query.constant),
                        "terms": terms,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({ "wires": wires })
        })
        .collect::<Vec<_>>();
    let report = serde_json::json!({
        "drive_wires": stats.wires,
        "compiled": stats.compiled,
        "fallback": stats.fallback,
        "declines": stats.declines,
        "shared_binary_nets": stats.shared_binary_nets,
        "shared_binary_wires": stats.shared_binary_wires,
        "all_guarded_15_nets": stats.all_guarded_15_nets,
        "shared_predicate_nets": stats.shared_predicate_nets,
        "multiple_predicate_nets": stats.multiple_predicate_nets,
        "other_drive_shape_nets": stats.other_drive_shape_nets,
        "channel_declines": stats.channel_declines,
        "transport_wires": prepared.graph.plans.instant_transport.nets.iter().map(|net| net.wires.len()).sum::<usize>(),
        "expression_work": stats.work,
        "representative_nets": representative_nets,
        "condition_table": program
            .conditions
            .iter()
            .enumerate()
            .map(|(id, condition)| serde_json::json!({ "id": id, "node": format!("{:?}", condition) }))
            .collect::<Vec<_>>(),
        "signal_channels": {
            "nets": channels.nets,
            "wires": channels.wires,
            "transitions": channels.transitions,
            "operations": channels.operations,
            "declines": channels.declines,
        },
    });
    let path = root().join("target/instant-drives/cpu-bubblesort-census.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();

    let transport_report = prepared.graph.instant_transport_rejection_report();
    let transport_stats = prepared.graph.plans.instant_transport.stats;
    for (reason, count) in [
        ("too_small", transport_stats.too_small),
        ("owner_overlap", transport_stats.owner_overlap),
        ("conditional_input", transport_stats.conditional_input),
        ("unsafe_receiver", transport_stats.unsafe_receiver),
        ("unsafe_input", transport_stats.unsafe_input),
        ("no_dust_dependency", transport_stats.no_dust_dependency),
        ("no_route_or_facts", transport_stats.no_route_or_facts),
        ("unsupported", transport_stats.unsupported),
    ] {
        assert_eq!(
            transport_report["rejected"][reason]["corridors"].as_u64().unwrap_or(0),
            count as u64,
            "census must match actual admission: {reason}"
        );
    }
    let transport_path = root().join("target/instant-drives/cpu-transport-rejections.json");
    std::fs::write(transport_path, serde_json::to_vec_pretty(&transport_report).unwrap()).unwrap();
    println!("BubbleSort transport rejection census: {}", transport_report["rejected_subreasons"]);
}

fn expand_condition(program: &InstantDriveProgram, id: u32) -> serde_json::Value {
    match &program.conditions[id as usize] {
        DriveCondition::Always => serde_json::json!("always"),
        DriveCondition::Never => serde_json::json!("never"),
        DriveCondition::State(test) => serde_json::json!({ "state": format!("{:?}", test) }),
        DriveCondition::WireEmits { node, face, raw_sides } => {
            serde_json::json!({ "wire_emits": { "node": node.index(), "face": format!("{:?}", face), "raw_sides": raw_sides } })
        }
        DriveCondition::Not(child) => serde_json::json!({ "not": expand_condition(program, *child) }),
        DriveCondition::All(children) => {
            serde_json::json!({ "all": children.iter().map(|&child| expand_condition(program, child)).collect::<Vec<_>>() })
        }
        DriveCondition::Any(children) => {
            serde_json::json!({ "any": children.iter().map(|&child| expand_condition(program, child)).collect::<Vec<_>>() })
        }
    }
}
