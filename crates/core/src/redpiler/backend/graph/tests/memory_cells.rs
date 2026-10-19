//! Candidate inventory only: QC and missing observer reset do not prove BUD memory.
use super::*;
use serde_json::json;
use std::collections::BTreeMap;

#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

#[test]
#[ignore = "research: ANPU active piston cost and prospective BUD-memory input/update inventory"]
fn anpu_memory_cell_opportunity_census() {
    let cpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "anpu_pong").unwrap();
    let mut world = cpus::load_cpu(cpu);
    world.disable_command_output_limits_for_replay();
    let reference: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/cpu-references/anpu_bud_updates.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(reference["schematic_sha256"], cpu.sha256);
    let reference_memory: FxHashSet<BlockPos> = serde_json::from_value(reference["memory"].clone()).unwrap();
    assert_eq!(reference_memory.len(), 896);
    let example = *reference_memory.iter().min_by_key(|pos| (pos.x, pos.y, pos.z)).unwrap();
    let example_column: Vec<_> = (-5..=5)
        .map(|dy| {
            let pos = BlockPos::new(example.x, example.y + dy, example.z);
            json!({"position":pos,"block":format!("{:?}",world.get_block(pos))})
        })
        .collect();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(5),
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
    let mechanisms = passes::utils::instant_mechanisms::discover_mechanisms(&prepared.graph, &monitor).unwrap();
    let reset_actors: FxHashSet<_> = mechanisms.iter().map(|mechanism| mechanism.piston).collect();
    let mut updates = vec![Vec::new(); prepared.graph.pistons.len()];
    for (route, spec) in prepared.graph.routes.iter().enumerate() {
        for (ordinal, delivery) in spec.deliveries.iter().enumerate() {
            let actor = match delivery.receiver {
                Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => Some(actor),
                Some(CallbackReceiver::Component(node)) => match prepared.graph.nodes[node.index()].kind {
                    CircuitNodeKind::Piston(actor) => Some(actor),
                    _ => None,
                },
                _ => None,
            };
            if let Some(actor) = actor {
                updates[actor.index()].push(json!({"route":route,"ordinal":ordinal,"source":spec.source.index(),
                    "guard":delivery.guard.index(),"kind":format!("{:?}",delivery.kind),"view":format!("{:?}",delivery.view)}));
            }
        }
    }
    let mut backend = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
    backend.test_recorder.memory_cell_activity = vec![[0; 2]; backend.program.pistons().len()];
    backend.use_block(cpu.origin + cpu.start).unwrap();
    for _ in 0..5000 {
        backend.tick().unwrap();
    }
    let mut histogram = BTreeMap::<String, [u64; 3]>::new();
    let mut rows = Vec::new();
    let mut totals = [0u64; 2];
    let mut reference_totals = [0u64; 3];
    let mut storage_totals = [0u64; 3];
    let mut storage_actors = FxHashSet::default();
    let mut pairs = Vec::new();
    for (index, spec) in backend.program.pistons().iter().enumerate() {
        let actor = PistonId::new(index as u32);
        let main = PowerTarget::Input {
            node: spec.node,
            port: InputPort::Main,
        };
        let quasi = PowerTarget::Input {
            node: spec.node,
            port: InputPort::Quasi,
        };
        let main_zero = match backend.program.fixed_signal_query(main) {
            Some((0, inputs)) => inputs.is_empty(),
            None => backend.program.power_links_to(main).is_empty(),
            _ => false,
        };
        let has_quasi = !backend.program.power_links_to(quasi).is_empty();
        let reset = reset_actors.contains(&actor);
        let category = if reset {
            "observer_reset_mechanism"
        } else if main_zero && has_quasi {
            "qc_only_without_recognized_reset"
        } else if has_quasi {
            "main_and_qc_without_recognized_reset"
        } else if main_zero {
            "neither_main_nor_qc"
        } else {
            "main_without_qc"
        };
        let activity = backend.test_recorder.memory_cell_activity[index];
        let inputs: Vec<_> = [main, quasi]
            .map(|target| {
                backend
                    .program
                    .power_links_to(target)
                    .iter()
                    .map(|link| {
                        let site = &backend.program.sites()[link.from.index()];
                        json!({"position":site.position,"source":format!("{:?}",site.source),"guard":link.guard.index(),
                        "port":format!("{:?}",link.from_port),"face":format!("{:?}",link.face)})
                    })
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .collect();
        let labeled_memory = reference_memory.contains(&spec.position);
        if labeled_memory {
            let candidates: Vec<_> = backend
                .program
                .pistons()
                .iter()
                .enumerate()
                .filter(|(_, upper)| {
                    upper.sticky
                        && upper.facing == mchprs_blocks::BlockFace::Bottom
                        && matches!(upper.lane, PistonLane::SingleResource { .. })
                        && backend
                            .program
                            .power_links_to(quasi)
                            .iter()
                            .any(|link| backend.program.sites()[link.from.index()].source == SiteSource::Contact(upper.far))
                })
                .collect();
            assert_eq!(candidates.len(), 1, "fixture read piston must have one storage payload input");
            let (storage, upper) = candidates[0];
            assert_eq!(upper.position, BlockPos::new(spec.position.x, spec.position.y + 4, spec.position.z));
            assert!(!reference_memory.contains(&upper.position));
            assert!(storage_actors.insert(storage), "storage/read pairs must be disjoint");
            let resource = backend.program.resource_for_piston(PistonId::new(storage as u32)).unwrap();
            assert_eq!(
                backend.program.profile_block(backend.program.resources()[resource.index()].material),
                Some(Block::RedstoneBlock)
            );
            storage_totals[0] += 1;
            for (total, value) in storage_totals[1..].iter_mut().zip(backend.test_recorder.memory_cell_activity[storage]) {
                *total += value;
            }
            pairs.push(json!({"storage_actor":storage,"read_actor":index,"storage_resource":resource.index()}));
            reference_totals[0] += 1;
            for (total, value) in reference_totals[1..].iter_mut().zip(activity) {
                *total += value;
            }
        }
        let count = histogram.entry(category.into()).or_default();
        count[0] += 1;
        for (total, value) in totals.iter_mut().zip(activity) {
            *total += value;
        }
        for (total, value) in count[1..].iter_mut().zip(activity) {
            *total += value;
        }
        rows.push(
            json!({"actor":index,"position":spec.position,"reference_memory_actor":labeled_memory,"main_proven_zero":main_zero,
            "has_quasi_input":has_quasi,"recognized_observer_reset":reset,"sticky":spec.sticky,
            "facing":format!("{:?}",spec.facing),"near":backend.program.contacts()[spec.near.index()].position,
            "far":backend.program.contacts()[spec.far.index()].position,"power_inputs_main_quasi":inputs,
            "lane":format!("{:?}",spec.lane),"activity":activity,"explicit_update_occurrences":updates[index]}),
        );
    }
    assert_eq!(
        totals,
        [backend.execution_counts().piston_samples, backend.execution_counts().piston_actions]
    );
    assert!(totals[0] > 0 && totals[1] > 0);
    assert_eq!(reference_totals[0] as usize, reference_memory.len());
    assert_eq!(storage_totals[0] as usize, reference_memory.len());
    let pair_totals = [
        storage_totals[0] + reference_totals[0],
        storage_totals[1] + reference_totals[1],
        storage_totals[2] + reference_totals[2],
    ];
    rows.sort_by_key(|row| std::cmp::Reverse(row["activity"][0].as_u64().unwrap()));
    let report = json!({"cpu":"anpu_pong","tier":5,"ticks":5000,"histogram_columns":["actors","power_samples","accepted_actions"],
        "histogram":histogram,"activity_columns":["power_samples","accepted_actions"],"totals":totals,"actors":rows,
        "reference_memory":{"source":"test_data/cpu-references/anpu_bud_updates.json","columns":["actors","power_samples","accepted_actions"],"totals":reference_totals,"claim":"Fixture labels select lower read actors. Each QC input is linked to one upper redstone-block storage payload; this is not a general recognition or replacement proof.","example_column":example_column,
            "storage_totals":storage_totals,"paired_totals":pair_totals,"pairs":pairs},
        "limits":"Opportunity only, not BUD recognition or a speed measurement. Missing observer provenance does not prove memory. Explicit route occurrences are not a complete activation certificate: contact alternatives, captured Turbo notifications, shared ownership, ordinary push/reset behavior, event order and handoff require proof. Uses current Graph without replacement."});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/anpu-memory-cell-opportunity.json");
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!(
        "ANPU MEMORY OPPORTUNITY totals={totals:?}, reference read={reference_totals:?}, storage={storage_totals:?}, paired={pair_totals:?}, categories={histogram:?}"
    );
}
