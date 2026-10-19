//! Large-device admission diagnostics, independent of the executable-layer certificate.
use super::*;
use crate::redpiler::passes::utils::instant_mechanisms::{discover_mechanisms, index_power};

fn counter_fixture() -> Value {
    let bytes = std::fs::read(root().join("test_data/instant-pistons-io/fixtures/counter_basic.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn rilax_bud_storage_never_becomes_instant_computation() {
    let fixture = manifest("rilax_memory_bank_bud");
    let (world, bounds) = load(&fixture);
    let monitor = TaskMonitor::default();
    let graph = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default())
        .unwrap()
        .graph;
    let catalog = crate::redpiler::analysis::instant::boundaries::infer_boundaries(&graph, &monitor).unwrap();
    let memory = fixture["observations"]["memory"].as_array().unwrap();
    assert_eq!(memory.len(), 64);
    for position in memory {
        let position = local_pos(position);
        let actor = PistonId::new(graph.pistons.iter().position(|actor| actor.position == position).unwrap() as u32);
        assert!(
            catalog.cells.iter().all(|cell| cell.uses.iter().all(|use_| use_.actor != actor)),
            "Rilax storage {position:?} stays outside"
        );
    }
}

#[test]
fn counter_keeps_all_sixteen_bud_cells_and_shared_reset_updates_at_the_boundary() {
    let fixture = counter_fixture();
    let (world, bounds) = load(&fixture);
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let graph = &prepared.graph;
    let mechanisms = discover_mechanisms(graph, &monitor).unwrap();
    let catalog = crate::redpiler::analysis::instant::boundaries::infer_boundaries(graph, &monitor).unwrap();
    let memory = fixture["ports"]["observations"]["memory"].as_array().unwrap();
    assert_eq!(memory.len(), 16);
    let owned_nodes: BTreeSet<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter().filter_map(|use_| use_.reset.observer()))
        .chain(catalog.corridors.iter().flat_map(|corridor| corridor.wires.iter().copied()))
        .collect();
    let mut reset_updates = 0;
    for position in memory {
        let position = local_pos(position);
        let actor = PistonId::new(graph.pistons.iter().position(|actor| actor.position == position).unwrap() as u32);
        assert!(
            mechanisms.iter().all(|mechanism| mechanism.piston != actor),
            "BUD data at {position:?} is not reset power"
        );
        assert!(catalog.cells.iter().all(|cell| cell.uses.iter().all(|use_| use_.actor != actor)));
        assert!(catalog.declines.iter().any(|decline| decline.actors.contains(&actor)));
        for (route, spec) in graph.routes.iter().enumerate() {
            if !owned_nodes.contains(&spec.source) {
                continue;
            }
            for (ordinal, delivery) in spec.deliveries.iter().enumerate() {
                if !matches!(delivery.kind, DeliveryKind::Update { .. })
                    || !matches!(delivery.receiver, Some(CallbackReceiver::PistonBase(receiver) | CallbackReceiver::PistonHead(receiver)) if receiver == actor)
                {
                    continue;
                }
                let occurrence = RouteOccurrence {
                    route: RouteId::new(route as u32),
                    ordinal: ordinal as u32,
                };
                assert!(
                    catalog
                        .ports
                        .iter()
                        .any(|port| port.direction == BoundaryDirection::Output && port.kind == BoundaryPortKind::Callback { occurrence }),
                    "shared reset must retain its memory update"
                );
                reset_updates += 1;
            }
        }
    }
    assert!(reset_updates >= 16, "each memory bit retains a qualifying shared-observer output");
}

#[test]
#[ignore = "explicit five-device instant admission and memory-boundary census"]
fn instant_region_admission_census() {
    for name in ["fpu_divider", "fpu_legal", "counter", "pc_counter", "cpu_bubblesort"] {
        let (world, bounds) = match name {
            "cpu_bubblesort" => {
                let (world, bounds, _) = super::large_builds::load_cpu(cpus::CPUS[2]);
                (world, bounds)
            }
            "counter" => load(&counter_fixture()),
            "pc_counter" => load(&pc_counter_contract::descriptor()),
            _ => load(&manifest(name)),
        };
        let monitor = TaskMonitor::default();
        monitor.set_budget_multiplier(8);
        let prepared = passes::rich::prepare(
            &world,
            bounds,
            world.scheduler().iter_entries().collect(),
            &monitor,
            AnalysisLimits::for_budget(8),
        )
        .unwrap();
        let graph = prepared.graph;
        let mechanisms = discover_mechanisms(&graph, &monitor).unwrap();
        let cells = crate::redpiler::analysis::instant::cells::discover_cells(&graph, &monitor).unwrap();
        let admitted = cells.cells.iter().flat_map(|cell| cell.uses.iter()).count();
        let admitted_set: BTreeSet<_> = cells.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect();
        let payload_resets = cells
            .cells
            .iter()
            .flat_map(|cell| cell.uses.iter())
            .filter(|use_| matches!(use_.reset, CellReset::Payload { .. }))
            .count();
        let inherited_resets = cells
            .cells
            .iter()
            .flat_map(|cell| &cell.uses)
            .filter(|use_| matches!(use_.reset, CellReset::InputRestoration { .. }))
            .count();
        let recognized: BTreeSet<_> = mechanisms.iter().map(|mechanism| mechanism.piston).collect();
        let incoming = index_power(&graph);
        let mut reasons = BTreeMap::<String, usize>::new();
        let mut examples = Vec::new();
        for (index, spec) in graph.pistons.iter().enumerate() {
            let actor = PistonId::new(index as u32);
            if recognized.contains(&actor) {
                continue;
            }
            let reason = format!(
                "sticky={} lane={}",
                spec.sticky,
                match spec.lane {
                    PistonLane::Empty => "empty",
                    _ => "resource",
                }
            );
            *reasons.entry(reason.clone()).or_default() += 1;
            if admitted_set.contains(&actor) || examples.len() >= 16 {
                continue;
            }
            let mut contributions = Vec::new();
            for port in [InputPort::Main, InputPort::Quasi] {
                for link in incoming.get(&PowerTarget::Input { node: spec.node, port }).into_iter().flatten() {
                    contributions.push(json!({"input":format!("{port:?}"),"source":graph.sites[link.from.index()].position,
                        "source_kind":format!("{:?}",graph.sites[link.from.index()].source),"port":format!("{:?}",link.from_port),
                        "guard":format!("{:?}",graph.guards[link.guard.index()]),"attenuation":link.attenuation}));
                }
            }
            let observers: Vec<_> = graph
                .nodes
                .iter()
                .enumerate()
                .filter_map(|(node, spec_node)| {
                    let CircuitNodeKind::Observer { watched, facing } = spec_node.kind else {
                        return None;
                    };
                    (matches!(graph.sites[watched.index()].source, SiteSource::Contact(contact) if contact == spec.base))
                        .then(|| json!({"node":node,"facing":format!("{facing:?}"),"position":spec_node.position}))
                })
                .collect();
            let payloads: Vec<_> = graph.resources.iter().enumerate().filter(|(_,resource)| resource.contacts.contains(&spec.near))
                .map(|(resource,spec)| json!({"resource":resource,"material":Block::from_id(graph.profiles[spec.material.index()].raw_state).get_name()})).collect();
            let feedback: Vec<_> = graph
                .power_links
                .iter()
                .filter(|link| graph.sites[link.from.index()].source == SiteSource::Contact(spec.near))
                .map(|link| format!("{link:?}"))
                .collect();
            let restore: Vec<_> = spec.routes.resource_restored_near.into_iter().flat_map(|route| graph.routes[route.index()].deliveries.iter())
                .filter(|delivery| matches!(delivery.kind,DeliveryKind::Update{..}) && delivery.receiver.is_some())
                .map(|delivery|json!({"receiver":format!("{:?}",delivery.receiver),"target":graph.sites[delivery.target.index()].position,"view":format!("{:?}",delivery.view),"guard":format!("{:?}",graph.guards[delivery.guard.index()])})).collect();
            examples.push(
                json!({"actor":index,"position":spec.position,"facing":format!("{:?}",spec.facing),"reason":reason,
                "observers":observers,"inputs":contributions,"payloads":payloads,"feedback":feedback,"restore_updates":restore}),
            );
        }
        let report = json!({"fixture":name,"actors":graph.pistons.len(),"recognized_observer_mechanisms":recognized.len(),"admitted_actors":admitted,
            "payload_reset_actors":payload_resets,"input_restoration_actors":inherited_resets,"outside_actors":graph.pistons.len()-admitted,"declines":format!("{:?}",cells.declined),"missing_observer_mechanisms":reasons,"examples":examples});
        save_report(&format!("admission-{name}"), &report);
        println!(
            "{name}: {admitted} / {} admitted actors ({payload_resets} payload resets, {inherited_resets} inherited resets), {} observer mechanisms; {reasons:?}",
            graph.pistons.len(),
            recognized.len()
        );
        assert_eq!(recognized.len() + reasons.values().sum::<usize>(), graph.pistons.len());
        assert!(!mechanisms.is_empty());
    }
}
