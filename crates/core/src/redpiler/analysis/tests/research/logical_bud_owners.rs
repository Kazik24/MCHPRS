use super::*;
use crate::redpiler::TaskMonitor;
use crate::redpiler::passes;
use crate::redpiler::program_graph::{CompileGraph, LogicalBudEventCondition, LogicalBudEventCut, PistonId, compile_logical_bud_owners};

fn compile_rilax(level: usize) -> CompileGraph {
    let fixture = manifest("rilax_memory_bank_bud");
    let (world, bounds) = load(&fixture);
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(4);
    let ticks = world.scheduler().iter_entries().collect();
    let mut prepared = passes::rich::prepare(&world, bounds, ticks, &monitor, AnalysisLimits::for_budget(4)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(level),
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    prepared.graph
}

#[test]
fn logical_bud_owner_pass_is_o6_only_and_binds_rilax_without_strict_reader_pair() {
    let low = compile_rilax(0);
    let mid = compile_rilax(5);
    assert!(low.plans.logical_bud_owners.is_empty());
    assert!(mid.plans.logical_bud_owners.is_empty());

    let graph = compile_rilax(6);
    let storage = graph.pistons.iter().position(|spec| spec.position == memory_pos(0, 0)).unwrap();
    let actor = PistonId::new(storage as u32);
    assert!(
        !graph
            .plans
            .bud_memory_reads
            .iter()
            .any(|read| read.storage == actor || read.reader == actor),
        "this storage owner is not admitted through the strict paired-reader recognizer"
    );
    let binding = graph
        .plans
        .logical_bud_owners
        .iter()
        .find(|owner| owner.actor == actor)
        .expect("O6 should bind the ordinary Rilax storage piston independently of BudMemoryRead");
    assert_eq!(binding.near, graph.pistons[storage].near);
    assert_eq!(binding.far, graph.pistons[storage].far);
    assert_eq!(graph.resources[binding.resource.index()].contacts.len(), 2);
    assert!(!binding.sample_routes.is_empty());
    assert!(
        binding.main_links.is_empty(),
        "an unlinked Main port is exact zero and does not disqualify this QC-only writer"
    );
    assert!(!binding.quasi_links.is_empty(), "the writer has an exact Quasi input binding");
    assert!(binding.transition.publication_delay_ticks == 2);
    assert!(binding.transition.events.iter().any(|event| {
        event.cut == LogicalBudEventCut::DeferredResourceNear && event.condition == LogicalBudEventCondition::ResourceRevisionCurrent
    }));
    assert!(binding.transition.events.iter().any(|event| {
        let delivery = graph.routes[event.occurrence.route.index()].deliveries[event.occurrence.ordinal as usize];
        match delivery.receiver {
            Some(
                crate::redpiler::program_graph::CallbackReceiver::PistonBase(other)
                | crate::redpiler::program_graph::CallbackReceiver::PistonHead(other),
            ) => other != actor,
            Some(crate::redpiler::program_graph::CallbackReceiver::Component(node)) => node != graph.pistons[storage].node,
            Some(crate::redpiler::program_graph::CallbackReceiver::Contact(_)) => true,
            None => false,
        }
    }));
}

#[test]
fn logical_bud_owner_rejects_shared_contacts_and_stale_plan_rows() {
    let mut graph = compile_rilax(6);
    let storage = graph.pistons.iter().position(|spec| spec.position == memory_pos(0, 0)).unwrap();
    let actor = PistonId::new(storage as u32);
    let owner = graph.plans.logical_bud_owners.iter().find(|owner| owner.actor == actor).unwrap().clone();

    let foreign = graph
        .pistons
        .iter()
        .enumerate()
        .find(|(index, _)| *index != storage)
        .map(|(index, _)| index)
        .unwrap();
    graph.pistons[foreign].base = owner.near;
    let discovered = compile_logical_bud_owners(&graph, &TaskMonitor::default()).unwrap();
    assert!(
        !discovered.iter().any(|candidate| candidate.actor == actor),
        "a foreign base invalidates exclusive payload ownership"
    );

    let mut forged = compile_rilax(6);
    let binding = forged.plans.logical_bud_owners.iter_mut().find(|owner| owner.actor == actor).unwrap();
    let event = binding
        .transition
        .events
        .iter_mut()
        .find(|event| event.occurrence.ordinal == 0)
        .expect("binding has an emitted callback row");
    event.occurrence.ordinal = u32::MAX;
    assert!(forged.validate().is_err(), "lowering must reject a forged callback occurrence");

    let mut forged_dispatch = compile_rilax(6);
    let binding = forged_dispatch
        .plans
        .logical_bud_owners
        .iter_mut()
        .find(|owner| owner.actor == actor)
        .unwrap();
    let event = binding.transition.events.first_mut().expect("binding has an emitted callback row");
    event.dispatch = match event.dispatch {
        crate::redpiler::program_graph::LogicalBudDispatch::OwnSample => crate::redpiler::program_graph::LogicalBudDispatch::Boundary,
        crate::redpiler::program_graph::LogicalBudDispatch::Boundary => crate::redpiler::program_graph::LogicalBudDispatch::OwnSample,
    };
    assert!(forged_dispatch.validate().is_err(), "lowering must reject forged typed dispatch rows");
}
