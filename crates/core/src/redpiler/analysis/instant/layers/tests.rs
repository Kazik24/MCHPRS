use super::*;
use mchprs_blocks::{BlockFace, BlockPos};

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

fn empty_graph(link: PowerLink) -> CompileGraph {
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

fn use_with_activations(activations: Box<[RouteOccurrence]>, base: RouteId, near: RouteId, far: Option<RouteId>) -> CellUse {
    CellUse {
        actor: PistonId::new(0),
        reset: NodeId::new(0),
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
                base_restored: RouteId::new(9),
                resource: None,
            },
            reset: CellResetWake {
                watched_actor: PistonId::new(0),
                on_base_changed: Box::new([]),
                on_base_restored: Box::new([]),
            },
        },
    }
}

fn occurrence(route: u32, ordinal: u32) -> RouteOccurrence {
    RouteOccurrence {
        route: RouteId::new(route),
        ordinal,
    }
}

fn compile_synthetic_layer(attenuation: u8) -> LayeredRegion {
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
    let observer = NodeId::new(2);
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
            far_changed: None,
            far_shape_changed: None,
            base_restored: RouteId::new(2),
            head_restored: RouteId::new(3),
            resource_restored_near: None,
            resource_restored_far: None,
        },
    }];
    graph
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
    assert_eq!(bindings.lifecycle[0].reason, LayerLifecycleReason::BasePendingSelfUpdate);
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
        vec![None],
    ];
    for receivers in cases {
        let graph = contact_activation_graph(&receivers);
        let use_ = use_with_activations(vec![occurrence(0, 0)].into_boxed_slice(), RouteId::new(0), RouteId::new(1), None);
        assert!(matches!(
            activation_bindings(&graph, &BoundaryCatalog::default(), 0, &use_).unwrap(),
            Err((LayerWitness::Callback(found), LayerDeclineReason::CallbackSemanticsUnsupported)) if found == occurrence(0, 0)
        ));
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

    let ordered = compile_outputs(&graph, &catalog, &component, &cell, &use_, &power_index, accepted, transfer, &mut builder)
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
        &power_index,
        accepted,
        transfer,
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
