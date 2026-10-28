//! Cold extraction probes for retaining feedback repeaters at event boundaries.
use super::instant_cpu_tests::cpus;
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::instant::layers::logical::compile_event_boundary;
use crate::redpiler::analysis::instant::layers::logical::compile_event_boundary_cut_feedback;
use crate::redpiler::analysis::instant::layers::logical::diagnostic_source_cycle_edges;
use crate::world::World;
use std::sync::Arc;

fn prepare_o6_cpu(cpu: cpus::Cpu) -> (cpus::Cpu, crate::plot::PlotWorld, passes::rich::PreparedGraph) {
    let mut world = cpus::load_cpu(cpu);
    if cpu.name == "cpu_bubblesort" {
        let sign = cpu.origin + BlockPos::new(186, 2, 184);
        assert!(matches!(world.get_block(sign), Block::WallSign { .. }));
        assert!(world.set_block(sign, Block::Air));
        world.flush_block_changes();
    }
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        ..Default::default()
    };
    let bounds = world.get_corners();
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
    (cpu, world, prepared)
}

fn probe_actor_roots(cpu: cpus::Cpu, prepared: &passes::rich::PreparedGraph, position: BlockPos, label: &str) {
    let actor = prepared
        .graph
        .pistons
        .iter()
        .position(|piston| piston.position == position)
        .map(|index| PistonId::new(index as u32))
        .expect("the selected native BUD actor must be present in the graph");
    let node = prepared.graph.pistons[actor.index()].node;
    let targets = [
        PowerTarget::Input { node, port: InputPort::Main },
        PowerTarget::Input {
            node,
            port: InputPort::Quasi,
        },
    ];
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    match compile_event_boundary(&prepared.graph, &targets, &monitor) {
        Ok(circuit) => eprintln!(
            "{} {label} actor={actor:?} targets={targets:?} inputs={} first_inputs={:?} expressions={} outputs={:?} declines={:?}",
            cpu.name,
            circuit.program.inputs.len(),
            circuit.program.inputs.iter().take(8).collect::<Vec<_>>(),
            circuit.program.expressions.len(),
            circuit.outputs,
            circuit.declines,
        ),
        Err(error) => eprintln!("{} {label} actor={actor:?} targets={targets:?} extraction declined: {error}", cpu.name),
    }
    dump_actor_state_and_sources(cpu.name, &prepared.graph, actor, label);
}

fn dump_actor_state_and_sources(name: &str, graph: &CompileGraph, actor: PistonId, label: &str) {
    let piston = graph.pistons[actor.index()];
    let state = graph.initial_pistons[actor.index()];
    eprintln!(
        "{name} {label} actor={actor:?} pos={:?} facing={:?} sticky={} lane={:?} initial={state:?} base={:?}@{:?} near={:?}@{:?} far={:?}@{:?}",
        piston.position,
        piston.facing,
        piston.sticky,
        piston.lane,
        piston.base,
        graph.contacts[piston.base.index()].position,
        piston.near,
        graph.contacts[piston.near.index()].position,
        piston.far,
        graph.contacts[piston.far.index()].position,
    );

    for port in [InputPort::Main, InputPort::Quasi] {
        let target = PowerTarget::Input { node: piston.node, port };
        let links: Vec<_> = graph.power_links.iter().filter(|link| link.to == target).collect();
        for link in links {
            let site = graph.sites[link.from.index()];
            let source = match site.source {
                SiteSource::Component(node) => format!(
                    "component {node:?} kind={:?} pos={:?} state={:?}",
                    graph.nodes[node.index()].kind,
                    graph.nodes[node.index()].position,
                    graph.nodes[node.index()].state,
                ),
                SiteSource::Contact(contact) => format!("contact {contact:?} pos={:?}", graph.contacts[contact.index()].position,),
                SiteSource::Constant(profile) => format!("constant {profile:?}"),
            };
            eprintln!(
                "{name} {label} {port:?} link from_site={:?}@{:?} strength={} source=({source}) from_port={:?} attenuation={} guard={:?}",
                link.from,
                site.position,
                site.source_strength,
                link.from_port,
                link.attenuation,
                graph.guards[link.guard.index()],
            );
        }
    }

    let mut touched_resources = Vec::new();
    for (index, resource) in graph.resources.iter().enumerate() {
        let id = ResourceId::new(index as u32);
        let contact_touched = resource
            .contacts
            .iter()
            .any(|contact| [piston.base, piston.near, piston.far].contains(contact));
        if contact_touched {
            touched_resources.push(id);
        }
    }
    for &resource in &touched_resources {
        let spec = &graph.resources[resource.index()];
        let current = graph.initial_resources[resource.index()];
        let contacts = spec
            .contacts
            .iter()
            .map(|&contact| (contact, graph.contacts[contact.index()].position))
            .collect::<Vec<_>>();
        let owners = graph
            .pistons
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                spec.contacts
                    .iter()
                    .any(|contact| [candidate.base, candidate.near, candidate.far].contains(contact))
                    .then_some(PistonId::new(index as u32))
            })
            .collect::<Vec<_>>();
        eprintln!("{name} {label} resource={resource:?} initial={current:?} contacts={contacts:?} lane_owners={owners:?}");
        for owner in &owners {
            let spec = graph.pistons[owner.index()];
            eprintln!(
                "{name} {label} resource={resource:?} owner={owner:?} pos={:?} facing={:?} initial={:?}",
                spec.position,
                spec.facing,
                graph.initial_pistons[owner.index()],
            );
        }
    }

    if let Some(catalog) = graph.plans.instant.boundaries.as_ref() {
        for (cell_index, cell) in catalog.cells.iter().enumerate() {
            let includes_actor = cell.uses.iter().any(|use_| use_.actor == actor);
            let includes_resource = cell.resources.iter().any(|pin| touched_resources.contains(&pin.resource));
            if !includes_actor && !includes_resource {
                continue;
            }
            let owners = cell
                .uses
                .iter()
                .map(|use_| {
                    let owner = graph.pistons[use_.actor.index()];
                    (use_.actor, owner.position, use_.reset.clone())
                })
                .collect::<Vec<_>>();
            let resources = cell
                .resources
                .iter()
                .map(|pin| (pin.resource, pin.source, pin.owners.to_vec(), pin.selection))
                .collect::<Vec<_>>();
            eprintln!(
                "{name} {label} catalog_cell={cell_index} actor_match={includes_actor} resource_match={includes_resource} owners={owners:?} resource_pins={resources:?}"
            );
        }
    }
}

#[test]
#[ignore = "research: cold O6 event-boundary extraction for ordinary ANPU and BubbleSort BUD actors"]
fn o6_event_boundary_actor_roots_report_cold_extraction() {
    let anpu = cpus::CPUS.iter().copied().find(|cpu| cpu.name == "anpu_pong").unwrap();
    let (anpu, _world, prepared) = prepare_o6_cpu(anpu);
    probe_actor_roots(anpu, &prepared, BlockPos::new(125, 53, 123), "upper-writer");

    let bubblesort = cpus::CPUS.iter().copied().find(|cpu| cpu.name == "cpu_bubblesort").unwrap();
    let (bubblesort, _world, prepared) = prepare_o6_cpu(bubblesort);
    probe_actor_roots(bubblesort, &prepared, BlockPos::new(177, 45, 64), "RAM-actor");
    let resource = ResourceId::new(1060);
    let spec = &prepared.graph.resources[resource.index()];
    eprintln!(
        "{} CPU cold-probe resource={resource:?} initial={:?} contacts={:?}",
        bubblesort.name,
        prepared.graph.initial_resources[resource.index()],
        spec.contacts
            .iter()
            .map(|&contact| (contact, prepared.graph.contacts[contact.index()].position))
            .collect::<Vec<_>>(),
    );
    for (cell_index, cell) in prepared.graph.plans.instant.boundaries.as_ref().unwrap().cells.iter().enumerate() {
        if cell.resources.iter().any(|pin| pin.resource == resource) {
            let owners = cell
                .uses
                .iter()
                .map(|use_| {
                    let piston = prepared.graph.pistons[use_.actor.index()];
                    (use_.actor, piston.position, use_.reset.clone())
                })
                .collect::<Vec<_>>();
            let pins = cell
                .resources
                .iter()
                .map(|pin| (pin.resource, pin.source, pin.owners.to_vec(), pin.selection))
                .collect::<Vec<_>>();
            eprintln!(
                "{} CPU resource owner catalog_cell={cell_index} owners={owners:?} pins={pins:?}",
                bubblesort.name
            );
        }
    }
}

fn run_live_acceptance_probe(cpu: cpus::Cpu, position: BlockPos, ticks: u32, label: &str) {
    let (cpu, mut world, prepared) = prepare_o6_cpu(cpu);
    if cpu.name == "anpu_pong" {
        world.disable_command_output_limits_for_replay();
    }
    let actor = prepared
        .graph
        .pistons
        .iter()
        .position(|piston| piston.position == position)
        .map(|index| PistonId::new(index as u32))
        .expect("selected actor must exist in the prepared graph");
    let node = prepared.graph.pistons[actor.index()].node;
    let targets = [
        PowerTarget::Input { node, port: InputPort::Main },
        PowerTarget::Input {
            node,
            port: InputPort::Quasi,
        },
    ];
    let monitor = Arc::new(TaskMonitor::default());
    monitor.set_budget_multiplier(8);
    let (circuit, cut_resources, cut_owners) = match compile_event_boundary_cut_feedback(&prepared.graph, &targets, &monitor) {
        Ok(result) => result,
        Err(error) => panic!("{} {label}: expected event-boundary feedback compilation to succeed: {error}", cpu.name),
    };
    let resource_rows = cut_resources
        .iter()
        .map(|resource| {
            let current = prepared.graph.initial_resources[resource.index()];
            let contacts = prepared.graph.resources[resource.index()]
                .contacts
                .iter()
                .map(|&contact| (contact, prepared.graph.contacts[contact.index()].position))
                .collect::<Vec<_>>();
            (*resource, current, contacts)
        })
        .collect::<Vec<_>>();
    let owner_rows = cut_owners
        .iter()
        .map(|owner| {
            let piston = prepared.graph.pistons[owner.index()];
            (*owner, piston.position, prepared.graph.initial_pistons[owner.index()])
        })
        .collect::<Vec<_>>();
    eprintln!(
        "{} {label}: actor={actor:?} cut_resources={} resource_rows={resource_rows:?} cut_owners={} owner_rows={owner_rows:?} inputs={} sources={:?} expressions={} outputs={:?} declines={:?}",
        cpu.name,
        cut_resources.len(),
        cut_owners.len(),
        circuit.program.inputs.len(),
        circuit.program.inputs.iter().map(|input| input.source).collect::<Vec<_>>(),
        circuit.program.expressions.len(),
        circuit.outputs.iter().map(|output| (output.target, output.kind)).collect::<Vec<_>>(),
        circuit.declines,
    );

    let mut backend = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
    backend
        .install_accepted_boundary_probe(actor, circuit)
        .unwrap_or_else(|error| panic!("{} {label}: cannot install accepted-boundary probe: {error}", cpu.name));

    backend.use_block(cpu.origin + cpu.start).unwrap();
    for tick in 1..=ticks {
        backend.tick_with_world(&mut world).unwrap();
        backend.flush(&mut world, false);
        if tick % 250 == 0 {
            eprintln!("{} {label}: traced {tick}/{ticks} active ticks", cpu.name);
        }
    }
    let report = backend.take_accepted_boundary_probe().expect("probe remains installed");
    eprintln!(
        "{} {label}: accepted={} Main match/mismatch={}/{} Quasi match/mismatch={}/{} Combined-OR match/mismatch={}/{} eval_errors={} last_error={:?}",
        cpu.name,
        report.total_accepted,
        report.main_matches,
        report.main_mismatches,
        report.quasi_matches,
        report.quasi_mismatches,
        report.combined_matches,
        report.combined_mismatches,
        report.evaluation_errors,
        report.last_error,
    );
    assert!(
        report.total_accepted > 0,
        "{} {label}: selected actor had no accepted actions in {ticks} ticks",
        cpu.name
    );
    assert_eq!(
        report.evaluation_errors, 0,
        "{} {label}: event-boundary input evaluation failed: {:?}",
        cpu.name, report.last_error
    );
    for (index, record) in report.records.iter().enumerate() {
        eprintln!(
            "{} {label}: cut#{index} tick={} phase={} action={:?} candidate=({},{}) actual_strength=({},{}) inputs={:?}",
            cpu.name,
            record.tick,
            record.phase,
            record.action,
            record.candidate_main,
            record.candidate_quasi,
            record.actual_main,
            record.actual_quasi,
            record.inputs,
        );
    }
    assert_eq!(
        report.combined_mismatches, 0,
        "{} {label}: Main||Quasi power differs at an accepted cut; first records are printed above",
        cpu.name,
    );
}

#[test]
#[ignore = "research: compare retained-source Main/Quasi equations at live ANPU accepted piston cuts"]
fn o6_selected_event_boundary_queries_report_live_acceptance_cuts_anpu() {
    let anpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "anpu_pong").unwrap();
    run_live_acceptance_probe(anpu, BlockPos::new(125, 53, 123), 1_000, "ANPU upper writer");
}

#[test]
#[ignore = "research: compare retained-source Main/Quasi equations at live BubbleSort accepted piston cuts"]
fn o6_selected_event_boundary_queries_report_live_acceptance_cuts_bubblesort() {
    let bubblesort = *cpus::CPUS.iter().find(|cpu| cpu.name == "cpu_bubblesort").unwrap();
    run_live_acceptance_probe(bubblesort, BlockPos::new(177, 45, 64), 1_000, "BubbleSort RAM actor");
}

#[test]
#[ignore = "research: post-propagation feedback edges for a BubbleSort RAM actor Main/Quasi query"]
fn o6_bubblesort_ram_query_reports_postpropagated_feedback_edges() {
    let cpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "cpu_bubblesort").unwrap();
    let (_, _world, prepared) = prepare_o6_cpu(cpu);
    let actor = prepared
        .graph
        .pistons
        .iter()
        .position(|piston| piston.position == BlockPos::new(177, 45, 64))
        .map(|index| PistonId::new(index as u32))
        .expect("selected BubbleSort RAM actor must exist");
    let node = prepared.graph.pistons[actor.index()].node;
    let targets = [
        PowerTarget::Input { node, port: InputPort::Main },
        PowerTarget::Input {
            node,
            port: InputPort::Quasi,
        },
    ];
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let report = diagnostic_source_cycle_edges(&prepared.graph, &targets, &monitor)
        .unwrap_or_else(|error| panic!("BubbleSort post-propagation cycle diagnostic failed: {error}"));
    assert!(!report.trim().is_empty(), "post-propagation cycle diagnostic must contain a result");
    eprintln!("cpu_bubblesort actor={actor:?} targets={targets:?}\n{report}");
}
