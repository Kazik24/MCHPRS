use super::*;

use mchprs_blocks::{BlockFace, BlockPos};

#[test]
fn route_lifecycle_preserves_order_phase_identity_and_budget_charges() {
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let mut source = callback_route(0);
    source.deliveries = vec![source.deliveries[0]; 2].into_boxed_slice();
    let mut lifecycle = Vec::new();
    let initial_work = builder.budget.work;
    for phase in [
        LayerLifecyclePhase::DeferredBaseRestoration,
        LayerLifecyclePhase::DeferredBaseRestoration,
        LayerLifecyclePhase::DeferredHeadRestoration,
    ] {
        append_route_lifecycle(&mut lifecycle, &source, RouteId::new(3), PistonId::new(0), phase, &mut builder).unwrap();
    }
    assert_eq!(builder.budget.work - initial_work, 6, "duplicate rows must still be charged");
    assert_eq!(lifecycle.len(), 4);
    assert_eq!(
        lifecycle.iter().map(|row| row.occurrence).collect::<Vec<_>>(),
        [occurrence(3, 0), occurrence(3, 1), occurrence(3, 0), occurrence(3, 1)]
    );
    assert!(lifecycle[..2].iter().all(|row| row.phase == LayerLifecyclePhase::DeferredBaseRestoration));
    assert!(lifecycle[2..].iter().all(|row| row.phase == LayerLifecyclePhase::DeferredHeadRestoration));
    monitor.cancel();
    assert!(matches!(
        append_route_lifecycle(&mut lifecycle, &source, RouteId::new(3), PistonId::new(0), LayerLifecyclePhase::ResetOutput, &mut builder),
        Err(ConstructionError::Invalid(message)) if message.contains("cancel")
    ));
    assert_eq!(lifecycle.len(), 4, "cancelled work must not append another row");
}

#[test]
fn proof_budget_limits_are_inclusive_and_cancellation_stays_fatal() {
    let monitor = TaskMonitor::default();
    let mut budget = LayerBudget {
        bytes: MAX_LAYER_BYTES - 1,
        work: MAX_LAYER_WORK - 1,
    };
    budget.retain(1).unwrap();
    budget.visit(&monitor).unwrap();
    assert_eq!(budget.retain(1), Err(ConstructionError::BudgetExceeded));
    assert_eq!(budget.visit(&monitor), Err(ConstructionError::BudgetExceeded));

    monitor.cancel();
    assert!(matches!(budget.visit(&monitor), Err(ConstructionError::Invalid(message)) if message.contains("cancel")));

    let graph = empty_graph(power_link(0));
    let catalog = BoundaryCatalog::default();
    let component = RegionComponent {
        cells: Box::new([]),
        corridors: Box::new([]),
        ports: Box::new([]),
    };
    for message in [
        "cancelled while compiling instant causal layers",
        "instant layer construction-work budget exceeded",
    ] {
        // Text from an invalid graph must never be interpreted as a recoverable budget error.
        assert!(matches!(
            component_result(&graph, &catalog, &component, Err(ConstructionError::Invalid(message.into()))),
            Err(error) if error == message
        ));
    }
}

#[test]
fn retained_budget_failure_does_not_publish_an_input_or_hide_malformed_expressions() {
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    builder.budget.bytes = MAX_LAYER_BYTES;
    let source = LayerInputSource::State(StateTest::PistonExtended(PistonId::new(0)));
    assert_eq!(
        builder.input(source, LayerInputRole::RequestSample),
        Err(ConstructionError::BudgetExceeded)
    );
    assert!(builder.inputs.is_empty());
    assert!(builder.input_ids.is_empty());
    assert!(matches!(builder.not(LayerExprId(u32::MAX)), Err(ConstructionError::Invalid(_))));
}

#[test]
fn shared_boolean_operands_are_charged_only_when_retained() {
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let roots: Vec<_> = (0..2)
        .map(|index| {
            let input = builder
                .input(
                    LayerInputSource::State(StateTest::PistonExtended(PistonId::new(index))),
                    LayerInputRole::RequestSample,
                )
                .unwrap();
            builder.input_expr(input).unwrap()
        })
        .collect();
    let before = builder.budget.bytes;
    let root = builder.all(roots.iter().copied()).unwrap();
    let retained = builder.budget.bytes;
    assert_eq!(
        retained - before,
        std::mem::size_of::<LayerExpr>() + 4 * std::mem::size_of::<LayerExprId>()
    );
    for _ in 0..16 {
        assert_eq!(builder.all(roots.iter().rev().copied()).unwrap(), root);
        assert_eq!(builder.budget.bytes, retained);
    }
    builder.budget.bytes = MAX_LAYER_BYTES;
    assert_eq!(builder.all(roots.iter().copied()).unwrap(), root);
    let expressions = builder.expressions.len();
    assert_eq!(builder.any(roots), Err(ConstructionError::BudgetExceeded));
    assert_eq!(builder.expressions.len(), expressions);
}

#[test]
fn boolean_builder_simplifications_preserve_roots_and_work_charges() {
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let false_root = LayerExprId(0);
    let true_root = LayerExprId(1);
    let input = builder
        .input(
            LayerInputSource::State(StateTest::PistonExtended(PistonId::new(0))),
            LayerInputRole::RequestSample,
        )
        .unwrap();
    let a = builder.input_expr(input).unwrap();
    let b_input = builder
        .input(
            LayerInputSource::State(StateTest::PistonExtended(PistonId::new(1))),
            LayerInputRole::RequestSample,
        )
        .unwrap();
    let b = builder.input_expr(b_input).unwrap();

    assert_eq!(builder.not(false_root).unwrap(), true_root);
    assert_eq!(builder.not(true_root).unwrap(), false_root);
    let not_a = builder.not(a).unwrap();
    assert_eq!(builder.not(not_a).unwrap(), a);
    assert_eq!(builder.all([]).unwrap(), true_root);
    assert_eq!(builder.any([]).unwrap(), false_root);

    let before = builder.budget.work;
    assert_eq!(builder.all([true_root, a]).unwrap(), a);
    assert_eq!(builder.budget.work - before, 2);
    let before = builder.budget.work;
    assert_eq!(builder.any([false_root, a]).unwrap(), a);
    assert_eq!(builder.budget.work - before, 2);
    assert_eq!(builder.all([a, a]).unwrap(), a);
    assert_eq!(builder.any([a, a]).unwrap(), a);
    assert_eq!(builder.all([a, not_a]).unwrap(), false_root);
    assert_eq!(builder.any([a, not_a]).unwrap(), true_root);
    assert_eq!(builder.all([a, b, false_root]).unwrap(), false_root);
    assert_eq!(builder.any([a, b, true_root]).unwrap(), true_root);
}

fn power_link(attenuation: u8) -> PowerLink {
    PowerLink {
        from: SiteId::new(0),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: NodeId::new(0),
            port: InputPort::Main,
        },
        face: BlockFace::Top,
        attenuation,
        guard: GuardId::new(0),
    }
}

pub(super) fn empty_graph(link: PowerLink) -> CompileGraph {
    CompileGraph {
        nodes: vec![],
        links: vec![],
        power_links: vec![link],
        comparator_inputs: vec![],
        profiles: vec![],
        guards: vec![Guard::Always],
        contacts: vec![],
        routes: vec![],
        wires: vec![],
        wire_relations: vec![],
        sites: vec![],
        pistons: vec![],
        resources: vec![],
        initial_pistons: vec![],
        initial_resources: vec![],
        plans: ExecutionPlans::default(),
    }
}

pub(super) fn use_with_activations(activations: Box<[RouteOccurrence]>, base: RouteId, near: RouteId, far: Option<RouteId>) -> CellUse {
    CellUse {
        actor: PistonId::new(0),
        reset: CellReset::Observer {
            node: NodeId::new(0),
            wake: CellResetWake {
                watched_actor: PistonId::new(0),
                on_base_changed: Box::new([]),
                on_base_restored: Box::new([]),
            },
        },
        powered: CellPinId(3),
        requested: CellPinId(5),
        valid: CellPinId(6),
        activations,
        retraction: CellRetractionEffect {
            actor_revision_increment: 1,
            transfer: None,
            routes: CellRetractionRoutes {
                base_changed: base,
                far_changed: far,
                near_changed: near,
            },
            wake: CellDeferredWake {
                ticks_after_acceptance: 2,
                base_restored: RouteId::new(3),
                resource: None,
            },
        },
    }
}

fn test_resource_pin() -> CellResourcePin {
    CellResourcePin {
        resource: ResourceId::new(0),
        source: ContactId::new(0),
        present: CellPinId(0),
        absent: CellPinId(1),
        owners: vec![PistonId::new(0)].into_boxed_slice(),
        selection: CellTokenSelection::FirstAcceptedPullRequestInFifo,
    }
}

fn test_resource_transfer() -> CellResourceTransfer {
    CellResourceTransfer {
        resource: ResourceId::new(0),
        source: ContactId::new(0),
        destination: ContactId::new(1),
        resource_revision_increment: 1,
        when_at_source: CellPinId(0),
    }
}

fn occurrence(route: u32, ordinal: u32) -> RouteOccurrence {
    RouteOccurrence {
        route: RouteId::new(route),
        ordinal,
    }
}

pub(super) fn compile_synthetic_layer(attenuation: u8) -> LayeredRegion {
    let link = power_link(attenuation);
    let monitor = TaskMonitor::default();
    let actor = PistonId::new(0);
    let events = [occurrence(7, 0), occurrence(8, 0)];
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let event_values = events
        .map(|occurrence| {
            let input = builder
                .input(
                    LayerInputSource::BoundaryEvent {
                        port: occurrence.route.index() as u32,
                        occurrence,
                    },
                    LayerInputRole::ActivationEvent,
                )
                .unwrap();
            builder.input_expr(input).unwrap()
        })
        .to_vec();
    let event = builder.any(event_values).unwrap();
    let sampled_power = builder
        .input(LayerInputSource::PowerContribution { port: 0, link }, LayerInputRole::RequestSample)
        .and_then(|input| builder.input_expr(input))
        .unwrap();
    let acceptance_power = builder
        .input(
            LayerInputSource::PowerContribution { port: 0, link },
            LayerInputRole::AcceptanceRevalidation,
        )
        .and_then(|input| builder.input_expr(input))
        .unwrap();
    let extended_request = builder
        .input(LayerInputSource::State(StateTest::PistonExtended(actor)), LayerInputRole::RequestSample)
        .and_then(|input| builder.input_expr(input))
        .unwrap();
    let ready_request = builder
        .input(
            LayerInputSource::State(StateTest::PistonAvailability {
                piston: actor,
                is: PistonAvailability::Ready,
            }),
            LayerInputRole::RequestSample,
        )
        .and_then(|input| builder.input_expr(input))
        .unwrap();
    let extended_acceptance = builder
        .input(
            LayerInputSource::State(StateTest::PistonExtended(actor)),
            LayerInputRole::AcceptanceRevalidation,
        )
        .and_then(|input| builder.input_expr(input))
        .unwrap();
    let ready_acceptance = builder
        .input(
            LayerInputSource::State(StateTest::PistonAvailability {
                piston: actor,
                is: PistonAvailability::Ready,
            }),
            LayerInputRole::AcceptanceRevalidation,
        )
        .and_then(|input| builder.input_expr(input))
        .unwrap();
    let not_sampled_power = builder.not(sampled_power).unwrap();
    let not_acceptance_power = builder.not(acceptance_power).unwrap();
    let requested = builder.all([event, extended_request, ready_request, not_sampled_power]).unwrap();
    let valid = builder.all([extended_acceptance, ready_acceptance, not_acceptance_power]).unwrap();
    let consumed_before = builder.constant(false).unwrap();
    let not_consumed = builder.not(consumed_before).unwrap();
    let accepted = builder.all([requested, valid, not_consumed]).unwrap();
    let consumed_after = builder.any([consumed_before, accepted]).unwrap();
    let stationary_entry = builder.all([extended_request, ready_request]).unwrap();
    LayeredRegion {
        id: LayerRegionId(0),
        cells: vec![0].into_boxed_slice(),
        corridors: Box::new([]),
        stationary_entry,
        inputs: builder.inputs.into_boxed_slice(),
        outputs: Box::new([]),
        resource_outputs: Box::new([]),
        cold_lifecycle_occurrences: Box::new([]),
        graph_outputs: Box::new([]),
        expressions: builder.expressions.into_boxed_slice(),
        pin_values: vec![LayerPinValue {
            cell: 0,
            pin: CellPinId(6),
            value: valid,
        }]
        .into_boxed_slice(),
        layers: vec![CausalLayer {
            order: LayerOrder::Fifo,
            uses: vec![LayerUse {
                use_id: LayerUseId(0),
                actor,
                requested,
                accepted,
                consumed_before,
                consumed_after,
                cause: UseCause {
                    enqueue_triggers: events.clone().into(),
                    boundary_occurrences: events.into(),
                },
            }]
            .into_boxed_slice(),
        }]
        .into_boxed_slice(),
    }
}

fn evaluate(region: &LayeredRegion, events: [bool; 2], sampled: u8, accepted: u8, ready: [bool; 2], extended: [bool; 2]) -> Vec<bool> {
    let event_occurrences = [occurrence(7, 0), occurrence(8, 0)];
    let inputs: Vec<_> = region
        .inputs
        .iter()
        .map(|input| match input.source {
            LayerInputSource::BoundaryEvent { occurrence, .. } => {
                let index = event_occurrences.iter().position(|candidate| *candidate == occurrence).unwrap();
                events[index]
            }
            LayerInputSource::PowerContribution { link, .. } => {
                let strength = match input.role {
                    LayerInputRole::RequestSample => sampled,
                    LayerInputRole::AcceptanceRevalidation => accepted,
                    LayerInputRole::ActivationEvent => panic!("power is not an activation event"),
                };
                strength > link.attenuation
            }
            LayerInputSource::State(StateTest::PistonExtended(_)) => extended[phase_index(input.role)],
            LayerInputSource::State(StateTest::PistonAvailability {
                is: PistonAvailability::Ready,
                ..
            }) => ready[phase_index(input.role)],
            other => panic!("unexpected synthetic input {other:?}"),
        })
        .collect();
    let mut values: Vec<bool> = Vec::with_capacity(region.expressions.len());
    for expression in &region.expressions {
        values.push(match expression {
            LayerExpr::Constant(value) => *value,
            LayerExpr::Input(input) => inputs[input.0 as usize],
            LayerExpr::Not(inner) => !values[inner.0 as usize],
            LayerExpr::All(operands) => operands.iter().all(|operand| values[operand.0 as usize]),
            LayerExpr::Any(operands) => operands.iter().any(|operand| values[operand.0 as usize]),
        });
    }
    values
}

fn phase_index(role: LayerInputRole) -> usize {
    match role {
        LayerInputRole::RequestSample => 0,
        LayerInputRole::AcceptanceRevalidation => 1,
        LayerInputRole::ActivationEvent => panic!("a state read is not an activation event"),
    }
}

#[test]
fn layer_boolean_builder_preserves_state_and_power_guards_for_all_strengths_and_attenuations() {
    for attenuation in 0..=15 {
        let region = compile_synthetic_layer(attenuation);
        let use_ = &region.layers[0].uses[0];
        let valid_pin = region
            .pin_values
            .iter()
            .find(|pin| pin.pin == CellPinId(6))
            .expect("the compiled fixture retains its live valid pin")
            .value;
        assert_eq!(region.layers[0].uses.len(), 1, "two callback occurrences still describe one actor use");
        assert_eq!(use_.cause.boundary_occurrences.len(), 2);

        for sampled in 0..=15 {
            for accepted in 0..=15 {
                for ready in [[false, false], [false, true], [true, false], [true, true]] {
                    for extended in [[false, false], [false, true], [true, false], [true, true]] {
                        for events in [[false, false], [true, false], [false, true], [true, true]] {
                            let values = evaluate(&region, events, sampled, accepted, ready, extended);
                            let requested = events.into_iter().any(|event| event) && ready[0] && extended[0] && sampled <= attenuation;
                            let expected_accept = requested && ready[1] && extended[1] && accepted <= attenuation;
                            assert_eq!(values[use_.requested.0 as usize], requested);
                            assert_eq!(values[valid_pin.0 as usize], ready[1] && extended[1] && accepted <= attenuation);
                            assert_eq!(values[use_.accepted.0 as usize], expected_accept);
                            assert_eq!(values[use_.consumed_after.0 as usize], expected_accept);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn callback_request_can_be_cancelled_by_live_acceptance_power() {
    let region = compile_synthetic_layer(0);
    let use_ = &region.layers[0].uses[0];
    let values = evaluate(&region, [true, false], 0, 15, [true; 2], [true; 2]);

    assert!(values[use_.requested.0 as usize], "the original callback sampled an unpowered request");
    assert!(!values[use_.accepted.0 as usize], "the FIFO acceptance cut rechecks current power");
    assert!(!values[use_.consumed_after.0 as usize], "a cancelled request does not consume the use");
}

#[test]
fn live_readiness_and_pose_are_not_replaced_by_the_request_snapshot() {
    let region = compile_synthetic_layer(0);
    let use_ = &region.layers[0].uses[0];
    for (ready, extended) in [([true, false], [true, true]), ([true, true], [true, false])] {
        let values = evaluate(&region, [true, false], 0, 0, ready, extended);
        assert!(values[use_.requested.0 as usize]);
        assert!(!values[use_.accepted.0 as usize]);
        assert!(!values[use_.consumed_after.0 as usize]);
    }
}

fn callback_route(source: u32) -> Route {
    Route {
        source: NodeId::new(source),
        deliveries: vec![RouteDelivery {
            target: SiteId::new(0),
            kind: DeliveryKind::Update { direction: None },
            guard: GuardId::new(0),
            view: OperandView::Live,
            sequence: 0,
            receiver: None,
        }]
        .into_boxed_slice(),
    }
}

fn contact_activation_graph(receivers: &[Option<CallbackReceiver>]) -> CompileGraph {
    let mut graph = empty_graph(power_link(0));
    let actor = PistonId::new(0);
    let actor_node = NodeId::new(0);
    let contact = ContactId::new(0);
    let contact_node = NodeId::new(1);
    let position = BlockPos::new(0, 0, 0);
    graph.guards = vec![Guard::Always, Guard::State(StateTest::PistonExtended(actor))];
    graph.nodes = vec![
        CircuitNodeDraft {
            position: Some(position),
            raw_state: None,
            kind: CircuitNodeKind::Piston(actor),
            state: ComponentState::Specialized,
            output_route: None,
        },
        CircuitNodeDraft {
            position: Some(position),
            raw_state: None,
            kind: CircuitNodeKind::Contact(contact),
            state: ComponentState::Specialized,
            output_route: None,
        },
        CircuitNodeDraft {
            position: Some(position),
            raw_state: None,
            kind: CircuitNodeKind::Observer {
                watched: SiteId::new(0),
                facing: BlockFace::East,
            },
            state: ComponentState::Specialized,
            output_route: None,
        },
    ];
    graph.profiles = vec![Profile {
        raw_state: 0,
        registry_type: 0,
        solid: false,
        transparent: true,
        cube: false,
        electrical: ProfileElectrical::None,
    }];
    graph.sites = vec![SiteSpec {
        position,
        source: SiteSource::Contact(contact),
        source_strength: 0,
    }];
    graph.contacts = vec![ContactSpec {
        node: contact_node,
        position,
        variants: receivers
            .iter()
            .copied()
            .map(|receiver| ContactVariant {
                when: GuardId::new(0),
                profile: ProfileId::new(0),
                receiver,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }];
    graph.routes = vec![Route {
        source: actor_node,
        deliveries: vec![RouteDelivery {
            target: SiteId::new(0),
            kind: DeliveryKind::Update {
                direction: Some(BlockFace::East),
            },
            guard: GuardId::new(1),
            view: OperandView::Live,
            sequence: 0,
            receiver: None,
        }]
        .into_boxed_slice(),
    }];
    graph.routes.extend((1..=6).map(callback_route));
    graph.pistons = vec![PistonSpec {
        node: actor_node,
        position,
        facing: BlockFace::East,
        sticky: true,
        base: contact,
        near: contact,
        far: contact,
        lane: PistonLane::Empty,
        routes: PistonRoutes {
            base_changed: RouteId::new(0),
            near_changed: RouteId::new(1),
            far_changed: Some(RouteId::new(2)),
            far_shape_changed: None,
            base_restored: RouteId::new(3),
            head_restored: RouteId::new(4),
            resource_restored_near: Some(RouteId::new(5)),
            resource_restored_far: Some(RouteId::new(6)),
        },
    }];
    graph
}

fn callback_route_with_receiver(kind: DeliveryKind, receiver: Option<CallbackReceiver>) -> Route {
    Route {
        source: NodeId::new(0),
        deliveries: vec![RouteDelivery {
            target: SiteId::new(0),
            kind,
            guard: GuardId::new(1),
            view: OperandView::Live,
            sequence: 0,
            receiver,
        }]
        .into_boxed_slice(),
    }
}

fn append_inert_contact(graph: &mut CompileGraph, position: BlockPos) -> (ContactId, SiteId) {
    let contact = ContactId::new(graph.contacts.len() as u32);
    let node = NodeId::new(graph.nodes.len() as u32);
    let site = SiteId::new(graph.sites.len() as u32);
    graph.nodes.push(CircuitNodeDraft {
        position: Some(position),
        raw_state: None,
        kind: CircuitNodeKind::Contact(contact),
        state: ComponentState::Specialized,
        output_route: None,
    });
    graph.contacts.push(ContactSpec {
        node,
        position,
        variants: vec![ContactVariant {
            when: GuardId::new(0),
            profile: ProfileId::new(0),
            receiver: None,
        }]
        .into_boxed_slice(),
    });
    graph.sites.push(SiteSpec {
        position,
        source: SiteSource::Contact(contact),
        source_strength: 0,
    });
    (contact, site)
}

#[test]
fn guarded_own_contact_updates_are_retained_as_base_pending_lifecycle() {
    let actor = PistonId::new(0);
    let receivers = [
        None,
        Some(CallbackReceiver::PistonHead(actor)),
        Some(CallbackReceiver::Component(NodeId::new(0))),
    ];
    let graph = contact_activation_graph(&receivers);
    let use_ = use_with_activations(vec![occurrence(0, 0)].into_boxed_slice(), RouteId::new(0), RouteId::new(1), None);
    let bindings = activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap().unwrap();

    assert!(bindings.boundary.is_empty());
    assert_eq!(bindings.lifecycle.len(), 1);
    assert_eq!(bindings.lifecycle[0].occurrence, occurrence(0, 0));
    assert_eq!(bindings.lifecycle[0].phase, LayerLifecyclePhase::RetractionBaseChanged);
    assert_eq!(bindings.lifecycle[0].reason, LayerLifecycleReason::InertCellCallback);
    assert_eq!(
        graph.routes[0].deliveries[0].guard,
        GuardId::new(1),
        "the conditional row remains on the original route"
    );
}

#[test]
fn foreign_observer_and_nested_contact_variants_are_not_self_lifecycle() {
    let actor = PistonId::new(0);
    let cases = [
        vec![None, Some(CallbackReceiver::PistonBase(PistonId::new(1)))],
        vec![
            Some(CallbackReceiver::PistonHead(actor)),
            Some(CallbackReceiver::Component(NodeId::new(2))),
        ],
        vec![
            Some(CallbackReceiver::PistonHead(actor)),
            Some(CallbackReceiver::Contact(ContactId::new(1))),
        ],
    ];
    for receivers in cases {
        let graph = contact_activation_graph(&receivers);
        let use_ = use_with_activations(vec![occurrence(0, 0)].into_boxed_slice(), RouteId::new(0), RouteId::new(1), None);
        assert!(matches!(
            activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap(),
            Err((LayerWitness::Callback(found), LayerDeclineReason::CallbackSemanticsUnsupported)) if found == occurrence(0, 0)
        ));
    }

    let mut graph = contact_activation_graph(&[None]);
    graph.nodes.push(CircuitNodeDraft {
        position: Some(BlockPos::new(1, 0, 0)),
        raw_state: None,
        kind: CircuitNodeKind::Wire(0),
        state: ComponentState::Specialized,
        output_route: None,
    });
    graph.contacts[0].variants = vec![ContactVariant {
        when: GuardId::new(0),
        profile: ProfileId::new(0),
        receiver: Some(CallbackReceiver::Component(NodeId::new(3))),
    }]
    .into_boxed_slice();
    graph.routes[0].deliveries[0].kind = DeliveryKind::Shape { direction: BlockFace::East };
    graph.routes[0].deliveries[0].receiver = Some(CallbackReceiver::Contact(ContactId::new(0)));
    let use_ = use_with_activations(vec![occurrence(0, 0)].into_boxed_slice(), RouteId::new(0), RouteId::new(1), None);
    assert!(matches!(
        activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap(),
        Err((LayerWitness::Callback(found), LayerDeclineReason::CallbackSemanticsUnsupported)) if found == occurrence(0, 0)
    ));
}

#[test]
fn own_contact_shape_is_inert_but_foreign_shape_is_not_self_lifecycle() {
    let actor = PistonId::new(0);
    let occurrence = occurrence(0, 0);
    let mut graph = contact_activation_graph(&[None, Some(CallbackReceiver::PistonHead(actor))]);
    graph.routes[0].deliveries[0].kind = DeliveryKind::Shape { direction: BlockFace::East };
    graph.routes[0].deliveries[0].receiver = Some(CallbackReceiver::Contact(ContactId::new(0)));
    let use_ = use_with_activations(vec![occurrence].into_boxed_slice(), RouteId::new(0), RouteId::new(1), None);
    let bindings = activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap().unwrap();
    assert!(bindings.boundary.is_empty());
    assert_eq!(bindings.lifecycle.len(), 1);
    assert_eq!(bindings.lifecycle[0].occurrence, occurrence);
    assert_eq!(bindings.lifecycle[0].reason, LayerLifecycleReason::InertCellCallback);

    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let lifecycle = lifecycle_occurrences(&graph, &BoundaryCatalog::default(), 0, &use_, &[], &mut builder)
        .unwrap()
        .unwrap();
    assert!(lifecycle.iter().any(|row| row.occurrence == occurrence));

    let mut foreign = contact_activation_graph(&[None, Some(CallbackReceiver::PistonHead(PistonId::new(1)))]);
    foreign.routes[0].deliveries[0].kind = DeliveryKind::Shape { direction: BlockFace::East };
    foreign.routes[0].deliveries[0].receiver = Some(CallbackReceiver::Contact(ContactId::new(0)));
    assert!(matches!(
        activation_bindings(&foreign, &BoundaryCatalog::default(), 0, &use_).unwrap(),
        Err((LayerWitness::Callback(found), LayerDeclineReason::CallbackSemanticsUnsupported)) if found == occurrence
    ));
}

#[test]
fn recognized_reset_observer_shape_is_inert_and_foreign_or_wire_shapes_decline() {
    let reset = NodeId::new(2);
    let occurrence = occurrence(0, 0);
    let mut graph = contact_activation_graph(&[None]);
    graph.routes[0].deliveries[0].kind = DeliveryKind::Shape { direction: BlockFace::Top };
    graph.routes[0].deliveries[0].guard = GuardId::new(0);
    graph.routes[0].deliveries[0].receiver = Some(CallbackReceiver::Component(reset));
    let mut use_ = use_with_activations(vec![occurrence].into_boxed_slice(), RouteId::new(0), RouteId::new(1), None);
    if let CellReset::Observer { node, .. } = &mut use_.reset {
        *node = reset;
    }

    let bindings = activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap().unwrap();
    assert!(bindings.boundary.is_empty());
    assert_eq!(bindings.lifecycle.len(), 1);
    assert_eq!(bindings.lifecycle[0].occurrence, occurrence);
    assert_eq!(bindings.lifecycle[0].phase, LayerLifecyclePhase::RetractionBaseChanged);
    assert_eq!(bindings.lifecycle[0].reason, LayerLifecycleReason::InertCellCallback);

    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let lifecycle = lifecycle_occurrences(&graph, &BoundaryCatalog::default(), 0, &use_, &[], &mut builder)
        .unwrap()
        .unwrap();
    assert!(
        lifecycle
            .iter()
            .any(|row| row.occurrence == occurrence && row.phase == LayerLifecyclePhase::RetractionBaseChanged)
    );

    for receiver_kind in [
        CircuitNodeKind::Observer {
            watched: SiteId::new(0),
            facing: BlockFace::East,
        },
        CircuitNodeKind::Wire(0),
    ] {
        let mut unsupported = graph.clone();
        let foreign = NodeId::new(3);
        unsupported.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(1, 0, 0)),
            raw_state: None,
            kind: receiver_kind,
            state: ComponentState::Specialized,
            output_route: None,
        });
        unsupported.routes[0].deliveries[0].receiver = Some(CallbackReceiver::Component(foreign));
        assert!(matches!(
            activation_bindings(&unsupported, &BoundaryCatalog::default(), 0, &use_).unwrap(),
            Err((LayerWitness::Callback(found), LayerDeclineReason::CallbackSemanticsUnsupported)) if found == occurrence
        ));
    }
}

#[test]
fn private_far_contact_shape_with_only_inert_variants_is_cold_but_foreign_contact_declines() {
    let mut graph = contact_activation_graph(&[None]);
    let (far, far_site) = append_inert_contact(&mut graph, BlockPos::new(2, 0, 0));
    graph.pistons[0].far = far;
    graph.pistons[0].lane = PistonLane::SingleResource { far };
    graph.routes[1].source = graph.pistons[0].node;
    graph.routes[1].deliveries[0].target = far_site;
    graph.routes[1].deliveries[0].kind = DeliveryKind::Shape { direction: BlockFace::East };
    graph.routes[1].deliveries[0].receiver = Some(CallbackReceiver::Contact(far));
    let occurrence = occurrence(1, 0);
    let use_ = use_with_activations(
        vec![occurrence].into_boxed_slice(),
        RouteId::new(0),
        RouteId::new(1),
        Some(RouteId::new(2)),
    );
    let bindings = activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap().unwrap();
    assert!(bindings.boundary.is_empty());
    assert_eq!(bindings.lifecycle.len(), 1);
    assert_eq!(bindings.lifecycle[0].phase, LayerLifecyclePhase::RetractionNearChanged);
    assert_eq!(bindings.lifecycle[0].reason, LayerLifecycleReason::InertCellCallback);

    let mut foreign = graph.clone();
    let (foreign_contact, foreign_site) = append_inert_contact(&mut foreign, BlockPos::new(3, 0, 0));
    foreign.routes[1].deliveries[0].target = foreign_site;
    foreign.routes[1].deliveries[0].receiver = Some(CallbackReceiver::Contact(foreign_contact));
    assert!(matches!(
        activation_bindings(&foreign, &BoundaryCatalog::default(), 0, &use_).unwrap(),
        Err((LayerWitness::Callback(found), LayerDeclineReason::CallbackSemanticsUnsupported)) if found == occurrence
    ));
}

#[test]
fn private_far_contact_update_with_only_inert_variants_is_cold_but_foreign_contact_declines() {
    let mut graph = contact_activation_graph(&[None]);
    let (far, far_site) = append_inert_contact(&mut graph, BlockPos::new(2, 0, 0));
    graph.pistons[0].far = far;
    graph.pistons[0].lane = PistonLane::SingleResource { far };
    graph.routes[1].source = graph.pistons[0].node;
    graph.routes[1].deliveries[0].target = far_site;
    graph.routes[1].deliveries[0].kind = DeliveryKind::Update {
        direction: Some(BlockFace::West),
    };
    graph.routes[1].deliveries[0].receiver = None;
    let occurrence = occurrence(1, 0);
    let use_ = use_with_activations(
        vec![occurrence].into_boxed_slice(),
        RouteId::new(0),
        RouteId::new(1),
        Some(RouteId::new(2)),
    );
    let bindings = activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap().unwrap();
    assert!(bindings.boundary.is_empty());
    assert_eq!(bindings.lifecycle.len(), 1);
    assert_eq!(bindings.lifecycle[0].occurrence, occurrence);
    assert_eq!(bindings.lifecycle[0].phase, LayerLifecyclePhase::RetractionNearChanged);
    assert_eq!(bindings.lifecycle[0].reason, LayerLifecycleReason::InertCellCallback);

    let mut foreign = graph.clone();
    let (_, foreign_site) = append_inert_contact(&mut foreign, BlockPos::new(3, 0, 0));
    foreign.routes[1].deliveries[0].target = foreign_site;
    assert!(matches!(
        activation_bindings(&foreign, &BoundaryCatalog::default(), 0, &use_).unwrap(),
        Err((LayerWitness::Callback(found), LayerDeclineReason::CallbackSemanticsUnsupported)) if found == occurrence
    ));
}

#[test]
fn resource_restoration_near_and_far_callbacks_stay_in_cold_lifecycle() {
    let actor = PistonId::new(0);
    let graph = contact_activation_graph(&[None, Some(CallbackReceiver::PistonHead(actor))]);
    let occurrences = [occurrence(5, 0), occurrence(6, 0)];
    let mut use_ = use_with_activations(occurrences.into(), RouteId::new(0), RouteId::new(1), Some(RouteId::new(2)));
    use_.retraction.wake.resource = Some(CellResourceWake {
        resource: ResourceId::new(0),
        contact: ContactId::new(0),
        route: RouteId::new(5),
    });

    for route in [RouteId::new(5), RouteId::new(6)] {
        assert_eq!(
            deferred_route_phase(&graph, graph.pistons[0].routes, &use_, route),
            Some(LayerLifecyclePhase::DeferredResourceRestoration)
        );
    }
    let bindings = activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap().unwrap();
    assert!(bindings.boundary.is_empty());
    assert_eq!(bindings.lifecycle.len(), 2);
    assert!(
        bindings.lifecycle.iter().all(|row| {
            row.phase == LayerLifecyclePhase::DeferredResourceRestoration && row.reason == LayerLifecycleReason::OrdinaryGraphLifecycle
        })
    );

    let catalog = BoundaryCatalog {
        ports: occurrences
            .iter()
            .map(|&occurrence| BoundaryPort {
                owner: BoundaryOwner::Cell(0),
                direction: BoundaryDirection::Output,
                kind: BoundaryPortKind::Callback { occurrence },
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        ..BoundaryCatalog::default()
    };
    let component = RegionComponent {
        cells: vec![0].into_boxed_slice(),
        corridors: Box::new([]),
        ports: vec![0, 1].into_boxed_slice(),
    };
    let monitor = TaskMonitor::default();
    let mut lifecycle_builder = LayerBuilder::new(&monitor).unwrap();
    let lifecycle = lifecycle_occurrences(&graph, &catalog, 0, &use_, &bindings.lifecycle, &mut lifecycle_builder)
        .unwrap()
        .unwrap();
    assert!(
        occurrences
            .iter()
            .all(|occurrence| lifecycle.iter().any(|row| row.occurrence == *occurrence))
    );

    let cell = CellContract {
        uses: Box::new([]),
        pins: Box::new([]),
        stationary_entry: CellPinId(0),
        resources: Box::new([]),
    };
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let outputs = compile_outputs(
        &graph,
        &catalog,
        &component,
        &cell,
        &use_,
        &test_resource_pin(),
        test_resource_transfer(),
        &index_power(&graph),
        LayerExprId(0),
        LayerExprId(1),
        &lifecycle,
        &mut builder,
    )
    .unwrap()
    .unwrap();
    assert!(outputs.is_empty(), "each skipped restoration output has a cold lifecycle row");
}

#[test]
fn reset_output_and_far_shape_routes_are_retained_as_exact_later_lifecycle_rows() {
    let actor = PistonId::new(0);
    let reset = NodeId::new(2);
    let mut graph = contact_activation_graph(&[None, Some(CallbackReceiver::PistonHead(actor))]);
    graph.nodes[reset.index()].output_route = Some(RouteId::new(7));
    let mut reset_route = callback_route_with_receiver(DeliveryKind::Update { direction: None }, None);
    reset_route.source = reset;
    let mut shape_route = callback_route_with_receiver(
        DeliveryKind::Shape { direction: BlockFace::East },
        Some(CallbackReceiver::Contact(ContactId::new(0))),
    );
    shape_route.source = graph.pistons[0].node;
    graph.routes.push(reset_route);
    graph.routes.push(shape_route);
    graph.pistons[0].routes.far_shape_changed = Some(RouteId::new(8));
    let activations = [occurrence(7, 0), occurrence(8, 0)];
    let mut use_ = use_with_activations(activations.into(), RouteId::new(0), RouteId::new(1), Some(RouteId::new(2)));
    if let CellReset::Observer { node, .. } = &mut use_.reset {
        *node = reset;
    }

    let bindings = activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap().unwrap();
    assert!(bindings.boundary.is_empty());
    assert_eq!(bindings.lifecycle.len(), 2);
    assert_eq!(bindings.lifecycle[0].phase, LayerLifecyclePhase::ResetOutput);
    assert_eq!(bindings.lifecycle[1].phase, LayerLifecyclePhase::ResetFarShapeChange);
    assert!(
        bindings
            .lifecycle
            .iter()
            .all(|row| row.reason == LayerLifecycleReason::OrdinaryGraphLifecycle)
    );

    let catalog = BoundaryCatalog {
        ports: activations
            .iter()
            .map(|&occurrence| BoundaryPort {
                owner: BoundaryOwner::Cell(0),
                direction: BoundaryDirection::Output,
                kind: BoundaryPortKind::Callback { occurrence },
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        ..BoundaryCatalog::default()
    };
    let component = RegionComponent {
        cells: vec![0].into_boxed_slice(),
        corridors: Box::new([]),
        ports: vec![0, 1].into_boxed_slice(),
    };
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let lifecycle = lifecycle_occurrences(&graph, &catalog, 0, &use_, &bindings.lifecycle, &mut builder)
        .unwrap()
        .unwrap();
    assert!(
        activations
            .iter()
            .all(|occurrence| lifecycle.iter().any(|row| row.occurrence == *occurrence))
    );

    let cell = CellContract {
        uses: Box::new([]),
        pins: Box::new([]),
        stationary_entry: CellPinId(0),
        resources: Box::new([]),
    };
    let outputs = compile_outputs(
        &graph,
        &catalog,
        &component,
        &cell,
        &use_,
        &test_resource_pin(),
        test_resource_transfer(),
        &index_power(&graph),
        LayerExprId(0),
        LayerExprId(1),
        &lifecycle,
        &mut builder,
    )
    .unwrap()
    .unwrap();
    assert!(outputs.is_empty(), "later Graph lifecycle rows are not finite-pull outputs");
}

fn evaluate_layer_root(expressions: &[LayerExpr], inputs: &[bool], root: LayerExprId) -> bool {
    let mut values: Vec<bool> = Vec::with_capacity(expressions.len());
    for expression in expressions {
        values.push(match expression {
            LayerExpr::Constant(value) => *value,
            LayerExpr::Input(input) => inputs[input.0 as usize],
            LayerExpr::Not(inner) => !values[inner.0 as usize],
            LayerExpr::All(operands) => operands.iter().all(|operand| values[operand.0 as usize]),
            LayerExpr::Any(operands) => operands.iter().any(|operand| values[operand.0 as usize]),
        });
    }
    values[root.0 as usize]
}

#[test]
fn guard_state_outputs_match_typed_post_pull_actor_and_resource_state() {
    let mut graph = empty_graph(power_link(0));
    graph.resources = vec![ResourceSpec {
        material: ProfileId::new(0),
        contacts: vec![ContactId::new(0), ContactId::new(1), ContactId::new(2)].into_boxed_slice(),
    }];
    let actor = PistonId::new(0);
    let resource = ResourceId::new(0);
    let source = ContactId::new(0);
    let destination = ContactId::new(1);
    let other = ContactId::new(2);
    let availability = [
        PistonAvailability::Ready,
        PistonAvailability::HeadPending,
        PistonAvailability::PublishingHeadRemoval,
        PistonAvailability::BasePendingHeadPresent,
        PistonAvailability::BasePending,
    ];
    let mut tests = vec![StateTest::PistonExtended(actor), StateTest::PistonHeadPresent(actor)];
    tests.extend(availability.map(|is| StateTest::PistonAvailability { piston: actor, is }));
    tests.push(StateTest::ResourceEnabled(resource));
    tests.extend([source, destination, other].map(|contact| StateTest::ResourceAt { resource, contact }));

    let catalog = BoundaryCatalog {
        ports: tests
            .iter()
            .copied()
            .map(|test| BoundaryPort {
                owner: BoundaryOwner::Cell(0),
                direction: BoundaryDirection::Output,
                kind: BoundaryPortKind::GuardState {
                    guard: GuardId::new(0),
                    test,
                },
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        ..BoundaryCatalog::default()
    };
    let component = RegionComponent {
        cells: vec![0].into_boxed_slice(),
        corridors: Box::new([]),
        ports: (0..tests.len() as u32).collect::<Vec<_>>().into_boxed_slice(),
    };
    let use_ = use_with_activations(Box::new([]), RouteId::new(1), RouteId::new(0), Some(RouteId::new(2)));
    let cell = CellContract {
        uses: Box::new([]),
        pins: Box::new([]),
        stationary_entry: CellPinId(0),
        resources: Box::new([]),
    };
    let pin = test_resource_pin();
    let transfer = test_resource_transfer();
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let accepted_input = builder
        .input(
            LayerInputSource::BoundaryEvent {
                port: 99,
                occurrence: occurrence(90, 0),
            },
            LayerInputRole::ActivationEvent,
        )
        .unwrap();
    let accepted = builder.input_expr(accepted_input).unwrap();
    let transfer_input = builder
        .input(
            LayerInputSource::BoundaryEvent {
                port: 100,
                occurrence: occurrence(91, 0),
            },
            LayerInputRole::ActivationEvent,
        )
        .unwrap();
    let transfer_when = builder.input_expr(transfer_input).unwrap();
    let outputs = compile_outputs(
        &graph,
        &catalog,
        &component,
        &cell,
        &use_,
        &pin,
        transfer,
        &index_power(&graph),
        accepted,
        transfer_when,
        &[],
        &mut builder,
    )
    .unwrap()
    .unwrap();
    assert_eq!(outputs.len(), tests.len());
    assert!(
        builder
            .inputs
            .iter()
            .all(|input| { !matches!(input.source, LayerInputSource::State(_)) || input.role == LayerInputRole::AcceptanceRevalidation })
    );

    for extended in [false, true] {
        for head_present in [false, true] {
            for current_availability in availability {
                for enabled in [false, true] {
                    for at in [source, destination, other] {
                        for request_accepted in [false, true] {
                            let accepted = request_accepted && extended && current_availability == PistonAvailability::Ready;
                            let transferred = accepted && enabled && at == source;
                            let inputs = builder
                                .inputs
                                .iter()
                                .map(|input| match input.source {
                                    LayerInputSource::BoundaryEvent { port: 99, .. } => accepted,
                                    LayerInputSource::BoundaryEvent { port: 100, .. } => transferred,
                                    LayerInputSource::State(test) => match test {
                                        StateTest::PistonExtended(_) => extended,
                                        StateTest::PistonHeadPresent(_) => head_present,
                                        StateTest::PistonAvailability { is, .. } => current_availability == is,
                                        StateTest::ResourceEnabled(_) => enabled,
                                        StateTest::ResourceAt { contact, .. } => at == contact,
                                    },
                                    other => panic!("unexpected state output input {other:?}"),
                                })
                                .collect::<Vec<_>>();
                            for output in &outputs {
                                let BoundaryPortKind::GuardState { test, .. } = catalog.ports[output.port as usize].kind else {
                                    panic!("state output keeps its original GuardState port")
                                };
                                let before = match test {
                                    StateTest::PistonExtended(_) => extended,
                                    StateTest::PistonHeadPresent(_) => head_present,
                                    StateTest::PistonAvailability { is, .. } => current_availability == is,
                                    StateTest::ResourceEnabled(_) => enabled,
                                    StateTest::ResourceAt { contact, .. } => at == contact,
                                };
                                let expected = match test {
                                    StateTest::PistonExtended(_) => before,
                                    StateTest::PistonHeadPresent(_) => before && !accepted,
                                    StateTest::PistonAvailability {
                                        is: PistonAvailability::BasePending,
                                        ..
                                    } => before || accepted,
                                    StateTest::PistonAvailability { .. } => before && !accepted,
                                    StateTest::ResourceEnabled(_) => before && !transferred,
                                    StateTest::ResourceAt { contact, .. } if contact == destination => before || transferred,
                                    StateTest::ResourceAt { .. } => before && !transferred,
                                };
                                assert_eq!(
                                    evaluate_layer_root(&builder.expressions, &inputs, output.emitted_when),
                                    expected,
                                    "post-pull state mismatch for {test:?} with actor {current_availability:?}, token {at:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn output_rows_follow_retraction_phase_and_retain_same_route_multiplicity() {
    let mut graph = empty_graph(power_link(0));
    graph.routes = vec![callback_route(0), callback_route(1), callback_route(2)];
    let occurrence = |route| RouteOccurrence {
        route: RouteId::new(route),
        ordinal: 0,
    };
    let catalog = BoundaryCatalog {
        cells: Box::new([]),
        declines: Box::new([]),
        corridors: Box::new([]),
        // Catalog storage order intentionally differs from base/Far/Near action order.
        ports: vec![
            BoundaryPort {
                owner: BoundaryOwner::Cell(0),
                direction: BoundaryDirection::Output,
                kind: BoundaryPortKind::Callback { occurrence: occurrence(0) },
            },
            BoundaryPort {
                owner: BoundaryOwner::Cell(0),
                direction: BoundaryDirection::Output,
                kind: BoundaryPortKind::Callback { occurrence: occurrence(2) },
            },
            BoundaryPort {
                owner: BoundaryOwner::Cell(0),
                direction: BoundaryDirection::Output,
                kind: BoundaryPortKind::Callback { occurrence: occurrence(1) },
            },
        ]
        .into_boxed_slice(),
    };
    let component = RegionComponent {
        cells: vec![0].into_boxed_slice(),
        corridors: Box::new([]),
        ports: vec![0, 1, 2].into_boxed_slice(),
    };
    let use_ = use_with_activations(Box::new([]), RouteId::new(1), RouteId::new(0), Some(RouteId::new(2)));
    let cell = CellContract {
        uses: Box::new([]),
        pins: Box::new([]),
        stationary_entry: CellPinId(0),
        resources: Box::new([]),
    };
    let accepted = LayerExprId(3);
    let transfer = LayerExprId(4);
    let power_index = index_power(&graph);
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();

    let ordered = compile_outputs(
        &graph,
        &catalog,
        &component,
        &cell,
        &use_,
        &test_resource_pin(),
        test_resource_transfer(),
        &power_index,
        accepted,
        transfer,
        &[],
        &mut builder,
    )
    .unwrap()
    .unwrap();
    assert_eq!(ordered.iter().map(|output| output.port).collect::<Vec<_>>(), [2, 1, 0]);
    assert_eq!(
        ordered.iter().map(|output| output.emitted_when).collect::<Vec<_>>(),
        [accepted, transfer, accepted]
    );

    let same_route_use = use_with_activations(Box::new([]), RouteId::new(0), RouteId::new(0), None);
    let same_route_component = RegionComponent {
        cells: vec![0].into_boxed_slice(),
        corridors: Box::new([]),
        ports: vec![0].into_boxed_slice(),
    };
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let duplicated = compile_outputs(
        &graph,
        &catalog,
        &same_route_component,
        &cell,
        &same_route_use,
        &test_resource_pin(),
        test_resource_transfer(),
        &power_index,
        accepted,
        transfer,
        &[],
        &mut builder,
    )
    .unwrap()
    .unwrap();
    assert_eq!(duplicated.iter().map(|output| output.port).collect::<Vec<_>>(), [0, 0]);
    assert_eq!(
        duplicated.iter().map(|output| output.emitted_when).collect::<Vec<_>>(),
        [accepted, accepted]
    );
}
