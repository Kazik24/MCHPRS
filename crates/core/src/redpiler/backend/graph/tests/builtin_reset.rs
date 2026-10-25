//! Built-in reset clocks preserve shared state, imported work and notification semantics.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::passes::GraphPass;

fn reset_graph() -> (CompileGraph, EntryState) {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(2),
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

fn compile_resets(graph: &mut CompileGraph, entry: &EntryState, monitor: &TaskMonitor) -> Result<(), String> {
    passes::compile_builtin_resets::CompileBuiltinResets.run(graph, &passes::PassContext::new(monitor, &CompilerOptions::default(), entry))
}

#[test]
fn builtin_resets_keep_one_clock_per_shared_driver_and_reject_output_circuits() {
    let (graph, entry) = reset_graph();
    let witnesses = passes::utils::instant_mechanisms::discover_mechanisms(&graph, &TaskMonitor::default()).unwrap();
    assert_eq!(witnesses.len(), 13);
    let mut compiled = graph.clone();
    compile_resets(&mut compiled, &entry, &TaskMonitor::default()).unwrap();
    assert_eq!(
        compiled
            .nodes
            .iter()
            .filter(|node| matches!(node.kind, CircuitNodeKind::BuiltinReset { .. }))
            .count(),
        9
    );
    assert_eq!(compiled.routes, graph.routes, "ordered ingress and captures remain intact");
    for (before, after) in graph.power_links.iter().zip(&compiled.power_links) {
        assert_eq!(
            (after.from, after.to, after.face, after.attenuation, after.guard),
            (before.from, before.to, before.face, before.attenuation, before.guard)
        );
        assert!(before.from_port == after.from_port || after.from_port == PowerPort::Output);
    }
    assert_eq!(compiled.initial_resources, graph.initial_resources);
    for witness in &witnesses {
        assert!(matches!(
            compiled.nodes[witness.observer.index()].kind,
            CircuitNodeKind::BuiltinReset { .. }
        ));
    }
    compiled.validate().unwrap();
    let once = compiled.clone();
    compile_resets(&mut compiled, &entry, &TaskMonitor::default()).unwrap();
    assert_eq!(compiled, once, "idempotent compilation");

    let observer = witnesses[0].observer;
    let mut exposed = graph.clone();
    let route = exposed.nodes[observer.index()].output_route.unwrap();
    let target = exposed
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(observer))
        .unwrap();
    let always = exposed.guards.iter().position(|guard| *guard == Guard::Always).unwrap();
    exposed.routes[route.index()].deliveries = vec![RouteDelivery {
        target: SiteId::new(target as u32),
        kind: DeliveryKind::Update {
            direction: Some(BlockFace::Top),
        },
        guard: GuardId::new(always as u32),
        view: OperandView::Live,
        sequence: 0,
        receiver: None,
    }]
    .into_boxed_slice();
    exposed.validate().unwrap();
    compile_resets(&mut exposed, &entry, &TaskMonitor::default()).unwrap();
    assert!(
        matches!(exposed.nodes[observer.index()].kind, CircuitNodeKind::Observer { .. }),
        "outside callbacks must not be swallowed"
    );
    let mut mismatched = graph.clone();
    mismatched.nodes[observer.index()].raw_state = Some(Block::Stone {}.get_id());
    // This deliberately changes electrical emission: discard all derived certificates.
    mismatched.clear_execution_plans();
    mismatched.validate().unwrap();
    compile_resets(&mut mismatched, &entry, &TaskMonitor::default()).unwrap();
    assert!(matches!(mismatched.nodes[observer.index()].kind, CircuitNodeKind::Observer { .. }));

    let mut invalid = once.clone();
    invalid.nodes[observer.index()].output_route = Some(route);
    assert!(matches!(invalid.validate(), Err(GraphBuildError::NodeKindMismatch { .. })));
    invalid = once;
    invalid.nodes[observer.index()].raw_state = Some(Block::Stone {}.get_id());
    assert!(matches!(invalid.validate(), Err(GraphBuildError::NodeKindMismatch { .. })));

    let mut cancelled = graph.clone();
    let monitor = TaskMonitor::default();
    monitor.cancel();
    assert!(compile_resets(&mut cancelled, &entry, &monitor).is_err());
    assert_eq!(cancelled, graph);
}

fn assert_clock_equivalent(general: &GraphBackend, builtin: &GraphBackend) {
    assert_eq!(builtin.state, general.state);
    assert_eq!(builtin.scheduled_ticks(), general.scheduled_ticks());
    assert_eq!(builtin.future.pending, general.future.pending);
    assert_eq!(builtin.dirty, general.dirty);
    assert_eq!(builtin.execution_counts(), general.execution_counts());
    for actor in general.program.pistons() {
        for port in [InputPort::Main, InputPort::Quasi] {
            assert_eq!(builtin.input_strength(actor.node, port), general.input_strength(actor.node, port));
        }
    }
}

#[test]
fn builtin_reset_notifications_and_imported_ticks_match_every_clock_transition() {
    let (graph, entry) = reset_graph();
    let mut compiled = graph.clone();
    compile_resets(&mut compiled, &entry, &TaskMonitor::default()).unwrap();
    let observer = compiled
        .nodes
        .iter()
        .position(|node| matches!(node.kind, CircuitNodeKind::BuiltinReset { .. }))
        .unwrap();
    let observer = NodeId::new(observer as u32);
    let site = compiled
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(observer))
        .unwrap();
    let site = SiteId::new(site as u32);
    let (general_program, general_state) = graph.lower().unwrap();
    let (builtin_program, builtin_state) = compiled.lower().unwrap();
    let mut cases = 0;
    for powered in [false, true] {
        for strength in 0..=15 {
            for direction in std::iter::once(None).chain(BlockFace::values().map(Some)) {
                for captured_power in [None, Some(false), Some(true)] {
                    for pending_case in 0..5 {
                        let mut general = GraphBackend::new_unactivated(general_program.clone(), general_state.clone(), 0).unwrap();
                        let mut builtin = GraphBackend::new_unactivated(builtin_program.clone(), builtin_state.clone(), 0).unwrap();
                        for backend in [&mut general, &mut builtin] {
                            let ComponentState::Electrical(state) = &mut backend.state.components[observer.index()] else {
                                unreachable!()
                            };
                            state.powered = powered;
                            state.output_strength = strength;
                            if pending_case != 0 {
                                let registry = backend.registry_type(observer);
                                let expected = if pending_case == 3 { Block::Stone {}.registry_id() } else { registry };
                                let tick = ImportedTick {
                                    target: Some(observer),
                                    position: backend.position(observer),
                                    block_type: (pending_case != 4).then_some(expected),
                                    delay: if pending_case == 1 { 0 } else { 2 },
                                    priority: if pending_case == 1 { TickPriority::NanoTick } else { TickPriority::High },
                                    current_registry_type: Some(registry),
                                };
                                backend
                                    .import_ticks(if pending_case == 2 { vec![tick, tick] } else { vec![tick] })
                                    .unwrap();
                            }
                        }
                        let captured = captured_power.map(|powered| ObservedSite {
                            profile: None,
                            receiver: Some(CallbackReceiver::Component(observer)),
                            properties: ObservedProperties::Electrical { powered, locked: false },
                        });
                        let kind = DeliveryKind::Update { direction };
                        if let Some(direction) = direction {
                            general
                                .update_component(observer, DeliveryKind::Shape { direction }, captured.as_ref())
                                .unwrap();
                            builtin
                                .update_component(observer, DeliveryKind::Shape { direction }, captured.as_ref())
                                .unwrap();
                            assert_clock_equivalent(&general, &builtin);
                        }
                        general.update_component(observer, kind, captured.as_ref()).unwrap();
                        builtin.update_component(observer, kind, captured.as_ref()).unwrap();
                        assert_clock_equivalent(&general, &builtin);
                        for port in BlockFace::values()
                            .into_iter()
                            .flat_map(|face| [PowerPort::Weak(face), PowerPort::Strong(face)])
                            .chain([
                                PowerPort::Stored,
                                PowerPort::Output,
                                PowerPort::Conducted { include_dust: false },
                                PowerPort::Conducted { include_dust: true },
                            ])
                        {
                            assert_eq!(builtin.site_power(site, port), general.site_power(site, port), "clock port {port:?}");
                        }
                        // Repeated notifications must retain pending multiplicity and insertion order.
                        general.update_component(observer, kind, captured.as_ref()).unwrap();
                        builtin.update_component(observer, kind, captured.as_ref()).unwrap();
                        for _ in 0..8 {
                            general.tick().unwrap();
                            builtin.tick().unwrap();
                            assert_clock_equivalent(&general, &builtin);
                        }
                        cases += 1;
                    }
                }
            }
        }
    }
    assert_eq!(cases, 3360);
}
