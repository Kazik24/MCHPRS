//! Ownership, instruction refinement, and real-fixture boundary replay.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::passes::Pass;
use crate::redpiler::passes::planning::PlanBuilder;

fn proxy_graph(id: &'static str) -> (CompileGraph, EntryState) {
    let circuit = load_adder(id, ORIGIN, None);
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
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
    (prepared.graph, prepared.entry)
}

#[test]
fn adder_regions_close_shared_payloads_and_resets_before_selecting_private_proxies() {
    let (graph, _) = proxy_graph("adder_1bit");
    let actors: Vec<_> = graph
        .plans
        .instant
        .regions
        .iter()
        .map(|r| r.actors.iter().map(|p| p.index()).collect::<Vec<_>>())
        .collect();
    let proxies: Vec<_> = graph.plans.instant.proxies.iter().map(|p| p.private_cells[0].actor.index()).collect();
    assert_eq!(actors, vec![vec![0, 2, 5, 8, 10], vec![1, 4, 7, 9], vec![3], vec![6], vec![11], vec![12]]);
    assert_eq!(proxies, vec![3, 6, 11, 12]);
    assert_eq!(actors.iter().map(Vec::len).sum::<usize>(), 13);
    assert!(!proxies.is_empty(), "the first private-cell family must execute");
    assert!(graph.plans.instant.regions.iter().any(|r| r.actors.len() > 1));
    for proxy in &graph.plans.instant.proxies {
        let region = &graph.plans.instant.regions[proxy.region];
        assert_eq!(region.actors.as_slice(), &[proxy.private_cells[0].actor]);
        assert_eq!(region.resources.as_slice(), &[proxy.private_cells[0].resource]);
        assert!(!region.incoming.is_empty());
        assert!(!region.outgoing.is_empty());
        assert!(!region.wire_readers.is_empty());
        assert!(
            proxy
                .routes
                .iter()
                .flat_map(|r| &r.steps)
                .any(|s| matches!(s.kind, InstantStepKind::ArmReset(_)))
        );
        for event in &region.outgoing {
            if let Some(route) = proxy.routes.iter().find(|route| route.route == event.route) {
                assert_eq!(
                    route.steps[event.ordinal as usize].kind,
                    InstantStepKind::Dispatch,
                    "outward events cannot be absorbed"
                );
            }
        }
    }
    let (program, _) = graph.lower().unwrap();
    for proxy in program.instant_proxies() {
        for route in &proxy.routes {
            assert!(program.routes()[route.route.index()].deliveries.is_empty());
        }
    }
}

#[test]
fn private_proxies_decline_uncertain_reset_ingress_and_are_idempotent() {
    use crate::redpiler::analysis::instant_regions::infer_proxies;
    use crate::redpiler::passes::PassContext;
    let (graph, entry) = proxy_graph("adder_1bit");
    let proxy = graph.plans.instant.proxies[0].clone();
    for captured in [false, true] {
        let mut changed = graph.clone();
        for route in &proxy.routes {
            for (ordinal, step) in route.steps.iter().enumerate() {
                if matches!(step.kind, InstantStepKind::ArmReset(_)) {
                    let delivery = &mut changed.routes[route.route.index()].deliveries[ordinal];
                    if captured {
                        delivery.view = OperandView::Captured;
                    } else {
                        delivery.kind = DeliveryKind::Update { direction: None };
                    }
                }
            }
        }
        let plans = infer_proxies(&changed, &changed.plans.instant.regions, &TaskMonitor::default()).unwrap();
        assert!(
            !plans
                .iter()
                .any(|candidate| candidate.private_cells[0].actor == proxy.private_cells[0].actor),
            "uncertain clock arming must stay general"
        );
    }
    let mut twice = graph.clone();
    let monitor = TaskMonitor::default();
    let context = PassContext {
        monitor: &monitor,
        options: &Default::default(),
        entry: &entry,
    };
    passes::identify_instant_regions::IdentifyInstantRegions
        .run(&mut twice, &context)
        .unwrap();
    passes::compile_instant_proxies::CompileInstantProxies.run(&mut twice, &context).unwrap();
    assert_eq!(twice, graph);
}

#[test]
fn private_resource_checks_match_general_checks_for_every_logical_availability() {
    let (graph, _) = proxy_graph("adder_1bit");
    let (program, state) = graph.lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    let proxies = backend.program.instant_proxies().to_vec();
    let availability = [
        PistonAvailability::Ready,
        PistonAvailability::HeadPending,
        PistonAvailability::PublishingHeadRemoval,
        PistonAvailability::BasePending,
        PistonAvailability::BasePendingHeadPresent,
    ];
    for plan in proxies {
        let proxy = plan.private_cells[0];
        let contacts = backend.program.resources()[proxy.resource.index()].contacts.clone();
        for &pose in &availability {
            for head_present in [false, true] {
                for extended in [false, true] {
                    for enabled in [false, true] {
                        for &at in &contacts {
                            backend.state.pistons[proxy.actor.index()].availability = pose;
                            backend.state.pistons[proxy.actor.index()].head_present = head_present;
                            backend.state.pistons[proxy.actor.index()].extended = extended;
                            backend.state.resources[proxy.resource.index()].at = at;
                            backend.state.resources[proxy.resource.index()].enabled = enabled;
                            for &target in &contacts {
                                for vacated in [None, Some(proxy.actor)] {
                                    assert_eq!(
                                        backend.resource_contact_available(proxy.resource, target, vacated),
                                        backend.general_resource_contact_available(proxy.resource, target, vacated),
                                        "actor {:?}, {pose:?}, head={head_present}, extended={extended}, enabled={enabled}, at={at:?}, target={target:?}",
                                        proxy.actor
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn instant_certificates_reject_corruption_and_unknown_passes_invalidate_them() {
    use crate::redpiler::passes::{GraphPass, PassContext, PassEntry, PassPipeline};
    let (graph, entry) = proxy_graph("adder_1bit");
    let mut invalid = graph.clone();
    invalid.plans.instant.regions[0].actors.pop();
    assert!(matches!(
        invalid.validate(),
        Err(GraphBuildError::InvalidInstantRegion { table: "regions", index: 0 })
    ));
    invalid = graph.clone();
    let proxy = &mut invalid.plans.instant.proxies[0];
    let step = proxy
        .routes
        .iter_mut()
        .flat_map(|r| r.steps.iter_mut())
        .find(|s| matches!(s.kind, InstantStepKind::ArmReset(_)))
        .unwrap();
    step.kind = InstantStepKind::Dispatch;
    assert!(matches!(
        invalid.validate(),
        Err(GraphBuildError::InvalidInstantRegion { table: "proxies", index: 0 })
    ));
    struct UnknownPass;
    impl GraphPass for UnknownPass {
        fn name(&self) -> &'static str {
            "unknown-pass"
        }
        fn run(&self, _: &mut CompileGraph, _: &PassContext<'_>) -> Result<(), String> {
            Ok(())
        }
    }
    let mut disabled = graph.clone();
    passes::run_graph_pipeline(
        &mut disabled,
        &Default::default(),
        &entry,
        &PassPipeline::new(vec![PassEntry {
            pass: Pass::rewrite(UnknownPass),
            enabled: false,
        }]),
        &TaskMonitor::default(),
    )
    .unwrap();
    assert_eq!(disabled, graph);
    passes::run_graph_pipeline(
        &mut disabled,
        &Default::default(),
        &entry,
        &PassPipeline::new(vec![PassEntry {
            pass: Pass::rewrite(UnknownPass),
            enabled: true,
        }]),
        &TaskMonitor::default(),
    )
    .unwrap();
    assert!(disabled.plans.instant.regions.is_empty() && disabled.plans.instant.proxies.is_empty());
    let mut cancelled = graph.clone();
    let monitor = TaskMonitor::default();
    monitor.cancel();
    assert!(
        passes::identify_instant_regions::IdentifyInstantRegions
            .run(
                &mut cancelled,
                &PassContext {
                    monitor: &monitor,
                    options: &Default::default(),
                    entry: &entry,
                }
            )
            .is_err()
    );
    assert_eq!(cancelled, graph, "cancelled discovery must publish nothing");
}

pub(super) fn assert_same_future(general: &GraphBackend, proxy: &GraphBackend) {
    assert_same_memory(general, proxy);
    assert_eq!(proxy.test_recorder.observed_operations, general.test_recorder.observed_operations);
    assert_eq!(proxy.test_recorder.trace, general.test_recorder.trace);
    let mut expected = general.execution_counts();
    expected.callbacks = proxy.execution_counts().callbacks;
    assert_eq!(proxy.execution_counts(), expected, "only absorbed dispatch counts may change");
}

pub(super) fn assert_same_memory(general: &GraphBackend, proxy: &GraphBackend) {
    assert_eq!(proxy.future.clock.logical_tick, general.future.clock.logical_tick);
    assert_eq!(proxy.future.clock.phase, general.future.clock.phase);
    assert_eq!(proxy.future.clock.scheduled_advanced, general.future.clock.scheduled_advanced);
    assert_eq!(proxy.state, general.state);
    assert_eq!(proxy.scheduled_ticks(), general.scheduled_ticks());
    assert_eq!(proxy.future.pending, general.future.pending);
    assert_eq!(
        proxy
            .future
            .piston_requests
            .iter()
            .map(|request| (request.piston, request.action))
            .collect::<Vec<_>>(),
        general
            .future
            .piston_requests
            .iter()
            .map(|request| (request.piston, request.action))
            .collect::<Vec<_>>()
    );
    let deferred = |backend: &GraphBackend| {
        backend
            .future
            .deferred_pistons
            .iter()
            .map(|event| {
                (
                    event.piston,
                    event.revision,
                    event.resource.map(|resource| (resource.resource, resource.revision, resource.contact)),
                    event.due_tick,
                    event.extending,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(deferred(proxy), deferred(general));
    assert_eq!(proxy.dirty, general.dirty);
}

#[test]
fn private_proxy_routes_refine_all_adder_states_and_pending_reset_obligations() {
    let (graph, _) = proxy_graph("adder_1bit");
    let mut general = graph.clone();
    general.plans.instant.proxies.clear();
    general.plans.instant.regions.clear();
    let (general_program, general_state) = general.lower().unwrap();
    let routes: Vec<_> = graph
        .plans
        .instant
        .proxies
        .iter()
        .flat_map(|proxy| proxy.routes.iter().map(|route| route.route))
        .collect();
    let (program, state) = graph.lower().unwrap();
    for phase in 0..32 {
        for &route in &routes {
            let mut general = GraphBackend::new(general_program.clone(), general_state.clone(), 0).unwrap();
            let mut proxy = GraphBackend::new(program.clone(), state.clone(), 0).unwrap();
            for backend in [&mut general, &mut proxy] {
                backend.test_recording = true;
                for (index, actor) in backend.state.pistons.iter_mut().enumerate() {
                    actor.extended = (phase >> (index % 5)) & 1 != 0;
                }
                for (index, resource) in backend.state.resources.iter_mut().enumerate() {
                    let contacts = &backend.program.resources()[index].contacts;
                    resource.at = contacts[(phase + index) % contacts.len()];
                    resource.enabled = (phase + index) % 3 != 0;
                }
                for index in 0..backend.state.components.len() {
                    if matches!(backend.program.nodes()[index].kind, CircuitNodeKind::BuiltinReset { .. }) {
                        let ComponentState::Electrical(state) = &mut backend.state.components[index] else {
                            unreachable!()
                        };
                        state.powered = phase & 1 != 0;
                        state.output_strength = if state.powered { 15 } else { 0 };
                        if phase & 2 != 0 {
                            backend.schedule(NodeId::new(index as u32), 1, TickPriority::Normal);
                        }
                    }
                }
            }
            general.deliver_route(route, None).unwrap();
            proxy.deliver_route(route, None).unwrap();
            assert_same_future(&general, &proxy);
            assert!(proxy.execution_counts().callbacks < general.execution_counts().callbacks);
        }
    }
}
