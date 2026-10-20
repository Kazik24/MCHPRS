//! Audit the complete physical interface before promising a standalone CPU kernel.
use super::*;
use crate::redpiler::analysis::instant::regions;
use rustc_hash::{FxHashMap, FxHashSet};

fn port_name(kind: BoundaryPortKind) -> &'static str {
    match kind {
        BoundaryPortKind::Power { .. } => "power",
        BoundaryPortKind::Callback { .. } => "callback",
        BoundaryPortKind::WireRelation { .. } => "wire_relation",
        BoundaryPortKind::Comparator { .. } => "comparator",
        BoundaryPortKind::GuardSite { .. } => "guard_site",
        BoundaryPortKind::GuardState { .. } => "guard_state",
        BoundaryPortKind::ContactAlternative { .. } => "contact_alternative",
        BoundaryPortKind::ObserverWatch { .. } => "observer_watch",
        BoundaryPortKind::CapturedWire { .. } => "captured_wire",
        BoundaryPortKind::WireNeighborhood { .. } => "wire_neighborhood",
        BoundaryPortKind::FixedRead { .. } => "fixed_read",
    }
}

#[test]
#[ignore = "explicit complete BubbleSort kernel/interface coverage audit"]
fn bubblesort_kernel_interface_is_complete_in_source_graph() {
    let cpu = cpus::CPUS[2];
    let (world, bounds, normalization) = large_builds::load_cpu(cpu);
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
    let partition = regions::partition(graph, catalog, &monitor).unwrap();
    let core_regions: Vec<_> = partition
        .regions
        .iter()
        .enumerate()
        .filter(|(_, region)| !region.cells.is_empty())
        .collect();
    assert_eq!(core_regions.len(), 1, "one connected computation candidate");
    assert_eq!(graph.plans.instant_events.len(), 1);
    let (core, region) = core_regions[0];
    let core = core as u32;
    let perimeter = regions::perimeters(graph, &partition, &monitor).unwrap();
    let ports = &perimeter[core as usize];
    let owner_region = |owner: Option<usize>| owner.map(|owner| partition.owner_region[owner]);
    let node_region = |node: NodeId| owner_region(partition.owners.nodes[node.index()]);
    let site_region = |site: SiteId| owner_region(partition.owners.sites[site.index()]);
    let mut missing = Vec::new();
    let mut crossing_power = 0usize;
    let mut crossing_callbacks = 0usize;
    for (id, link) in graph.power_links.iter().enumerate() {
        let source = site_region(link.from);
        let target = match link.to {
            PowerTarget::Input { node, .. } => node_region(node),
            PowerTarget::Conductor { site, .. } => site_region(site),
        };
        if (source == Some(core)) != (target == Some(core)) {
            crossing_power += 1;
            let direction = if target == Some(core) {
                BoundaryDirection::Input
            } else {
                BoundaryDirection::Output
            };
            if !ports.contains(&(direction, BoundaryPortKind::Power { link: id as u32 })) {
                missing.push(format!("power {id}"));
            }
        }
    }
    for (id, route) in graph.routes.iter().enumerate() {
        let source = node_region(route.source);
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            let target = match delivery.receiver {
                Some(CallbackReceiver::Component(node)) => node_region(node),
                Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) => {
                    owner_region(partition.owners.actors[actor.index()])
                }
                Some(CallbackReceiver::Contact(contact)) => owner_region(partition.owners.contacts[contact.index()]),
                None => site_region(delivery.target),
            };
            if (source == Some(core)) != (target == Some(core)) {
                crossing_callbacks += 1;
                let direction = if target == Some(core) {
                    BoundaryDirection::Input
                } else {
                    BoundaryDirection::Output
                };
                let kind = BoundaryPortKind::Callback {
                    occurrence: RouteOccurrence {
                        route: RouteId::new(id as u32),
                        ordinal: ordinal as u32,
                    },
                };
                if !ports.contains(&(direction, kind)) {
                    missing.push(format!("callback {id}:{ordinal}"));
                }
            }
        }
    }
    let mut ram_nodes = FxHashMap::default();
    let mut ram_contacts = FxHashMap::<ContactId, Vec<usize>>::default();
    let mut ram_actors = FxHashMap::default();
    let mut ram = Vec::new();
    for y in [7, 17, 27, 37] {
        for row in 0..16 {
            for bit in 0..12 {
                let position = cpu.origin + BlockPos::new(175 + 4 * bit, y, 62 - 4 * row);
                let (id, actor) = graph
                    .pistons
                    .iter()
                    .enumerate()
                    .find(|(_, actor)| actor.position == position)
                    .expect("every RAM bit must retain a physical state owner");
                let index = ram.len();
                ram_nodes.insert(actor.node, index);
                ram_actors.insert(PistonId::new(id as u32), index);
                for contact in [actor.base, actor.near, actor.far] {
                    ram_contacts.entry(contact).or_default().push(index);
                }
                ram.push(json!({"bank_y":y,"row":row,"bit":bit,"position":position,"actor":id,
                    "owner":if node_region(actor.node)==Some(core) {"kernel"} else {"external_state"}}));
            }
        }
    }
    let mut ram_ports = vec![FxHashSet::default(); ram.len()];
    for &(direction, kind) in ports {
        let mut affected = Vec::new();
        let mut add_site = |site: SiteId| match graph.sites[site.index()].source {
            SiteSource::Component(node) => affected.extend(ram_nodes.get(&node).copied()),
            SiteSource::Contact(contact) => affected.extend(ram_contacts.get(&contact).into_iter().flatten().copied()),
            SiteSource::Constant(_) => {}
        };
        match kind {
            BoundaryPortKind::Power { link } => {
                let link = graph.power_links[link as usize];
                add_site(link.from);
                match link.to {
                    PowerTarget::Input { node, .. } => affected.extend(ram_nodes.get(&node).copied()),
                    PowerTarget::Conductor { site, .. } => add_site(site),
                }
            }
            BoundaryPortKind::Callback { occurrence } => {
                let delivery = graph.routes[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
                add_site(delivery.target);
                if let Some(CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor)) = delivery.receiver {
                    affected.extend(ram_actors.get(&actor).copied());
                }
            }
            BoundaryPortKind::GuardState {
                test: StateTest::PistonExtended(actor) | StateTest::PistonHeadPresent(actor) | StateTest::PistonAvailability { piston: actor, .. },
                ..
            } => affected.extend(ram_actors.get(&actor).copied()),
            BoundaryPortKind::GuardSite { site, .. } | BoundaryPortKind::ObserverWatch { site, .. } | BoundaryPortKind::FixedRead { site, .. } => {
                add_site(site)
            }
            BoundaryPortKind::CapturedWire { neighbor, .. } | BoundaryPortKind::WireNeighborhood { neighbor, .. } => add_site(neighbor),
            BoundaryPortKind::ContactAlternative { contact, .. } => affected.extend(ram_contacts.get(&contact).into_iter().flatten().copied()),
            BoundaryPortKind::WireRelation { relation } => add_site(graph.wire_relations[relation as usize].to),
            _ => {}
        }
        for index in affected {
            ram_ports[index].insert((direction, kind));
        }
    }
    for (row, refs) in ram.iter_mut().zip(&ram_ports) {
        row["read_boundary_refs"] = json!(refs.iter().filter(|(direction, _)| *direction == BoundaryDirection::Input).count());
        row["write_or_notification_boundary_refs"] = json!(refs.iter().filter(|(direction, _)| *direction == BoundaryDirection::Output).count());
    }
    let controls: Vec<_> = graph.nodes.iter().enumerate().filter(|(_, node)| matches!(node.kind,
        CircuitNodeKind::Electrical(NodeType::Button | NodeType::Lever | NodeType::PressurePlate))).map(|(id, node)| json!({
            "node":id,"position":node.position,"kind":format!("{:?}",node.kind),"owner":if node_region(NodeId::new(id as u32))==Some(core) {"kernel"} else {"external_input"},
            "outgoing_routes":graph.routes.iter().filter(|route|route.source.index()==id).count(),
            "power_links":graph.power_links.iter().filter(|link|matches!(graph.sites[link.from.index()].source,SiteSource::Component(source) if source.index()==id)).count()
        })).collect();
    assert!(
        controls.iter().any(|control| control["position"] == json!(cpu.origin + cpu.start)),
        "start button is represented"
    );
    assert_eq!(controls.len(), 53, "every control in the frozen CPU fixture is retained");
    assert!(
        controls
            .iter()
            .all(|control| control["outgoing_routes"].as_u64().unwrap() > 0 || control["power_links"].as_u64().unwrap() > 0),
        "every control must retain its source dependencies"
    );
    let peripherals: Vec<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(id, node)| {
            node_region(NodeId::new(*id as u32)) != Some(core) && !matches!(node.kind, CircuitNodeKind::Wire(_) | CircuitNodeKind::Contact(_))
        })
        .map(|(id, node)| json!({"node":id,"position":node.position,"kind":format!("{:?}",node.kind)}))
        .collect();
    let mut peripheral_kinds = BTreeMap::<String, usize>::new();
    for node in &peripherals {
        *peripheral_kinds.entry(node["kind"].as_str().unwrap().into()).or_default() += 1;
    }
    let mut port_counts = BTreeMap::<String, usize>::new();
    for &(direction, kind) in ports {
        *port_counts.entry(format!("{direction:?}/{}", port_name(kind))).or_default() += 1;
    }
    let outside_wires: Vec<_> = graph
        .wires
        .iter()
        .filter(|wire| node_region(wire.node) != Some(core))
        .map(|wire| {
            json!({
                "node":wire.node.index(),"position":graph.nodes[wire.node.index()].position,"region":node_region(wire.node),
                "has_output_route":graph.nodes[wire.node.index()].output_route.is_some(),"turbo_route":wire.turbo_route.map(|id|id.index())
            })
        })
        .collect();
    let report = json!({"device":"CPU_BUBBLESORT","schematic":cpu.schematic,"sha256":cpu.sha256,"normalization":normalization,
        "composition_candidates":core_regions.len(),"event_kernel_groups":graph.plans.instant_events.len(),
        "candidate_pistons":graph.plans.instant_events[0].actors.len(),"total_pistons":graph.pistons.len(),
        "candidate_corridors":region.corridors.len(),"candidate_wires":region.corridors.iter().map(|&id|catalog.corridors[id as usize].wires.len()).sum::<usize>(),
        "runtime_kernel_owned_wires":graph.plans.instant_events[0].wires.len(),"crossing_power_links":crossing_power,"crossing_callback_occurrences":crossing_callbacks,
        "total_wires":graph.wires.len(),"outside_wires":outside_wires,
        "missing_crossing_edges":missing,"boundary_port_counts":port_counts,"controls":controls,"ram":ram,
        "peripheral_kinds":peripheral_kinds,"peripheral_state_and_io_nodes":peripherals,
        "total_resources":graph.resources.len(),"kernel_resources":partition.owners.resources.iter().filter(|&&owner|owner_region(owner)==Some(core)).count(),
        "ram_without_boundary_refs":ram_ports.iter().filter(|refs|refs.is_empty()).count(),
        "standalone_accelerating_kernel_implemented":false,
        "remaining_interface_lowering":["explicit memory read/write transactions from the retained piston/state/callback dependencies",
            "explicit scheduled-tick and deferred-piston effects; current boundary rows retain references to their physical node implementations",
            "install and execute the merged perimeter in a dense kernel; current event entries only reduce piston acceptance"]});
    let path = root().join("target/instant-kernel-interface");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("cpu-bubblesort-interface.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(missing.is_empty(), "missing crossing edges: {missing:?}");
    assert_eq!(ram.len(), 768);
    assert!(
        ram_ports
            .iter()
            .all(|refs| refs.iter().any(|(direction, _)| *direction == BoundaryDirection::Input)
                && refs.iter().any(|(direction, _)| *direction == BoundaryDirection::Output)),
        "every RAM cell must retain both read and write/notification boundary references"
    );
    println!(
        "CPU interface: {} actors / {} wires in candidate; {} input controls; {} crossing power / {} callback edges; {} RAM states",
        report["candidate_pistons"],
        report["candidate_wires"],
        report["controls"].as_array().unwrap().len(),
        crossing_power,
        crossing_callbacks,
        report["ram"].as_array().unwrap().len()
    );
}
