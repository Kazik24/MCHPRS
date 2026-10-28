//! Read-only reports for a caller-selected interaction island; not an admission proof.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::backend::graph::GraphBackend;
use crate::redpiler::components::NodeType;
use crate::redpiler::passes;
use crate::redpiler::program_graph::{CircuitNodeKind, EntryState, PowerTarget, SiteSource};
use crate::world::World;
use mchprs_blocks::BlockPos;
use serde_json::{Value, json};

fn selected_boundary_report(
    graph: &CompileGraph,
    entry: &EntryState,
    trigger: BlockPos,
    declared: &[(String, BlockPos)],
    monitor: &TaskMonitor,
) -> Value {
    let partition = islands::partition(graph, monitor).expect("partition interaction graph");
    let trigger_index = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(trigger))
        .expect("declared trigger in graph");
    let island = &partition.islands[partition.node_island[trigger_index] as usize];
    let node_inside = |node: NodeId| island.nodes.binary_search(&node).is_ok();
    let site_inside = |site: SiteId| island.sites.binary_search(&site).is_ok();
    let mut internal_power = 0usize;
    let mut internal_callbacks = 0usize;
    let mut static_power = Vec::new();
    let mut ingress = Vec::new();
    let mut egress = Vec::new();
    for link in &graph.power_links {
        let from = &graph.sites[link.from.index()];
        let source = !matches!(from.source, SiteSource::Constant(_)) && site_inside(link.from);
        let target = match link.to {
            PowerTarget::Input { node, .. } => node_inside(node),
            PowerTarget::Conductor { site, .. } => site_inside(site),
        };
        let destination = match link.to {
            PowerTarget::Input { node, .. } => graph.nodes[node.index()].position,
            PowerTarget::Conductor { site, .. } => Some(graph.sites[site.index()].position),
        };
        let row = || json!({"kind":"power", "source":from.position, "source_kind":format!("{:?}",from.source), "destination":destination, "source_port":format!("{:?}",link.from_port), "target":format!("{:?}",link.to), "guard":format!("{:?}",link.guard), "attenuation":link.attenuation});
        if matches!(from.source, SiteSource::Constant(_)) && target {
            static_power.push(row());
            continue;
        }
        match (source, target) {
            (true, true) => internal_power += 1,
            (false, true) => ingress.push(row()),
            (true, false) => egress.push(row()),
            _ => {}
        }
    }
    for (route_index, route) in graph.routes.iter().enumerate() {
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            let source = node_inside(route.source);
            let target = site_inside(delivery.target);
            let row = || json!({"kind":"callback", "route":route_index, "ordinal":ordinal, "source":graph.nodes[route.source.index()].position, "destination":graph.sites[delivery.target.index()].position, "delivery":format!("{:?}",delivery.kind), "receiver":format!("{:?}",delivery.receiver), "guard":format!("{:?}",delivery.guard)});
            match (source, target) {
                (true, true) => internal_callbacks += 1,
                (false, true) => ingress.push(row()),
                (true, false) => egress.push(row()),
                _ => {}
            }
        }
    }
    let nodes = island
        .nodes
        .iter()
        .map(|id| {
            let node = &graph.nodes[id.index()];
            json!({"id":id.index(), "position":node.position, "kind":format!("{:?}",node.kind)})
        })
        .collect::<Vec<_>>();
    let resources = island.resources.iter().map(|id| {
        let resource = &graph.resources[id.index()];
        let state = graph.initial_resources[id.index()];
        json!({"id":id.index(), "initial_contact":graph.contacts[state.at.index()].position, "enabled":state.enabled, "possible_contacts":resource.contacts.iter().map(|contact| graph.contacts[contact.index()].position).collect::<Vec<_>>()})
    }).collect::<Vec<_>>();
    let mut quasi_counts = vec![0usize; graph.nodes.len()];
    for link in &graph.power_links {
        if let PowerTarget::Input {
            node,
            port: InputPort::Quasi,
        } = link.to
        {
            quasi_counts[node.index()] += 1;
        }
    }
    let pistons = island.pistons.iter().map(|id| {
        let piston = &graph.pistons[id.index()];
        let quasi_inputs = quasi_counts[piston.node.index()];
        json!({"id":id.index(), "base":piston.position, "near":graph.contacts[piston.near.index()].position, "far":graph.contacts[piston.far.index()].position, "sticky":piston.sticky, "quasi_inputs":quasi_inputs, "potential_bud":quasi_inputs > 0})
    }).collect::<Vec<_>>();
    let bud_reads = graph.plans.bud_memory_reads.iter().map(|read| json!({"storage":graph.pistons[read.storage.index()].position, "reader":graph.pistons[read.reader.index()].position, "storage_inside_island":island.pistons.binary_search(&read.storage).is_ok(), "reader_inside_island":island.pistons.binary_search(&read.reader).is_ok(), "source":graph.contacts[read.source.index()].position, "resource":read.resource.index()})).collect::<Vec<_>>();
    let clocks = super::timing::candidate_clocks(graph, monitor).expect("enumerate clocks").into_iter().filter(|id| island.pistons.binary_search(id).is_ok()).map(|id| {
        let piston = &graph.pistons[id.index()];
        match super::timing::infer_for_clock(graph, id, monitor) {
            Ok(timing) => json!({"clock":piston.position, "trigger":graph.nodes[timing.trigger.index()].position, "actors":timing.actor_specs.iter().map(|actor| json!({"base":graph.contacts[actor.base.index()].position, "near":graph.contacts[actor.near.index()].position, "far":graph.contacts[actor.far.index()].position})).collect::<Vec<_>>() }),
            Err(reason) => json!({"clock":piston.position,"decline":reason}),
        }
    }).collect::<Vec<_>>();
    let planned_owners = graph.plans.logical_devices.iter().map(|device| json!({"component":device.scope.component, "owned_nodes":device.scope.owned_nodes.iter().filter_map(|id| graph.nodes[id.index()].position).collect::<Vec<_>>(), "owned_contacts":device.scope.owned_contacts.iter().map(|id| graph.contacts[id.index()].position).collect::<Vec<_>>(), "owned_resources":device.scope.owned_resources.iter().map(|id| id.index()).collect::<Vec<_>>() })).collect::<Vec<_>>();
    let interfaces = graph.nodes.iter().enumerate().filter_map(|(index, node)| {
        let category = match node.kind {
            CircuitNodeKind::Electrical(NodeType::Lever) => "lever",
            CircuitNodeKind::Electrical(NodeType::Button) => "button",
            CircuitNodeKind::Electrical(NodeType::PressurePlate) => "pressure_plate",
            CircuitNodeKind::Electrical(NodeType::Lamp) => "lamp",
            CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => "repeater",
            CircuitNodeKind::Electrical(NodeType::CommandBlock { .. }) => "command_block",
            CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. } => "observer_or_reset",
            _ => return None,
        };
        Some(json!({"category":category, "position":node.position, "inside_island":partition.node_island[index] == island.id, "kind":format!("{:?}",node.kind), "entry_state":format!("{:?}",node.state)}))
    }).collect::<Vec<_>>();
    json!({
        "status":"interaction island is a candidate only; not an ownership or activation proof",
        "selection":{"trigger":trigger,"island":island.id,"node_count":island.nodes.len(),"site_count":island.sites.len(),"contact_count":island.contacts.len(),"resource_count":island.resources.len(),"piston_count":island.pistons.len()},
        "declared_ports":declared.iter().map(|(name,pos)| json!({"name":name,"position":pos,"component_kind":graph.nodes.iter().find(|node| node.position == Some(*pos)).map(|node| format!("{:?}",node.kind)),"inside_island":graph.nodes.iter().enumerate().find(|(_,node)| node.position == Some(*pos)).is_some_and(|(i,_)| partition.node_island[i] == island.id)})).collect::<Vec<_>>(),
        "nodes":nodes, "interfaces":interfaces, "contacts":island.contacts.iter().map(|id| graph.contacts[id.index()].position).collect::<Vec<_>>(), "resources":resources, "pistons":pistons,
        "recognized_bud_reads":bud_reads, "potential_bud_definition":"piston with at least one quasi-power input; not proof of storage", "clocks":clocks,
        "internal_dependencies":{"power_links":internal_power,"callback_deliveries":internal_callbacks}, "static_power_sources":static_power, "dynamic_ingress":ingress, "dynamic_egress":egress,
        "planned_owners":planned_owners, "runtime_ownership":"not activated or verified", "pending_ticks":entry.pending_ticks.iter().map(|entry| json!({"position":entry.tick.pos,"ticks_left":entry.tick.ticks_left,"priority":format!("{:?}",entry.tick.tick_priority),"block_type":entry.tick.block_type})).collect::<Vec<_>>(),
        "commands":island.nodes.iter().filter_map(|id| { let node = &graph.nodes[id.index()]; matches!(node.kind, CircuitNodeKind::Electrical(NodeType::CommandBlock { .. })).then(|| json!({"position":node.position,"kind":format!("{:?}",node.kind)})) }).collect::<Vec<_>>(),
        "unrepresented_crossings":"dynamic crossings require explicit boundary proof; zero listed crossings is graph-derived only and does not prove isolation"
    })
}

#[test]
#[ignore = "external isolated FPU_27 schematic and expensive read-only graph analysis"]
fn isolated_fpu27_selected_boundary_report() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("test_data/piston-research/fixtures/fpu_legal.json")).unwrap()).unwrap();
    manifest["fixture"] = json!("target/build-audit-tps/prod/FPU_27-2026-10-27.schem");
    manifest["sha256"] = json!("b6182c6c0541d692d635d72d23ecc8f33cef2fb6071f4ba4be66485ff6892bc1");
    manifest["origin"] = json!([40, 30, 40]);
    let world = super::cpus::load_manifest(&manifest).unwrap();
    let origin = super::cpus::manifest_origin(&manifest);
    let ports = &manifest["ports"];
    let trigger = origin + super::cpus::manifest_position(&ports["trigger"]);
    let mut declared = vec![("trigger".to_owned(), trigger)];
    for name in [
        "light_blue_operand_inputs",
        "red_operand_inputs",
        "opcode_levers_by_sign_weight_4_2_1",
        "output_by_label_bit_15_to_0",
    ] {
        for pos in ports[name].as_array().unwrap() {
            declared.push((name.to_owned(), origin + super::cpus::manifest_position(pos)));
        }
    }
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        ..Default::default()
    };
    let ticks = world.scheduler().iter_entries().collect();
    let prepared = passes::rich::prepare(&world, world.get_corners(), ticks, &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let prepared = prepared
        .run_passes(&options, &passes::default_graph_pipeline(&options), &monitor)
        .unwrap()
        .into_prepared();
    let mut report = selected_boundary_report(&prepared.graph, &prepared.entry, trigger, &declared, &monitor);
    let backend = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
    report["runtime_ownership"] = json!({"installation":"constructed for inspection, not attached to a plot", "admission":format!("{:?}",backend.logical_device_admission()), "device_count":backend.logical_devices_count()});
    report["requested_observation"] =
        json!({"publication":"falling clock edge", "event_binding":"not yet proven", "intermediate_settling":"not required"});
    report["compiler_warnings"] = json!(monitor.warnings());
    report["analysis_options"] = json!({"optimization_level":6,"budget_multiplier":8,"stable_instant_inputs":true,"note":"read-only inspection; this is not default-budget admission"});
    report["auxiliary_observations"] = json!({
        "repeaters":manifest["observations"]["output_repeaters"].as_array().unwrap().iter().map(|pos| origin + super::cpus::manifest_position(pos)).collect::<Vec<_>>(),
        "repeater_scope":manifest["ports"]["output_repeaters_scope"],
        "other_control":manifest["observations"]["other_control"].as_array().unwrap().iter().map(|pos| origin + super::cpus::manifest_position(pos)).collect::<Vec<_>>()
    });
    report["fixture"] = json!({"manifest":"test_data/piston-research/fixtures/fpu_legal.json", "schematic":"target/build-audit-tps/prod/FPU_27-2026-10-27.schem", "sha256":manifest["sha256"], "origin":manifest["origin"]});
    report["world_piston_entry"] = json!({"phase":format!("{:?}",world.piston_state().phase),"events":world.piston_state().events.iter().map(|event| format!("{event:?}")).collect::<Vec<_>>(),"motions":world.piston_state().motions.iter().map(|motion| motion.pos).collect::<Vec<_>>()});
    let output = root.join("target/fpu27-selected-boundary-report.json");
    std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("wrote {}", output.display());
    println!(
        "candidate island: {} nodes, {} pistons, {} resources; static power sources {}, dynamic ingress {}, dynamic egress {}; planned owners {}; entry ticks {}; clock candidates {}",
        report["selection"]["node_count"],
        report["selection"]["piston_count"],
        report["selection"]["resource_count"],
        report["static_power_sources"].as_array().unwrap().len(),
        report["dynamic_ingress"].as_array().unwrap().len(),
        report["dynamic_egress"].as_array().unwrap().len(),
        report["planned_owners"].as_array().unwrap().len(),
        report["pending_ticks"].as_array().unwrap().len(),
        report["clocks"].as_array().unwrap().len()
    );
    assert_eq!(report["declared_ports"].as_array().unwrap().len(), 52);
}
