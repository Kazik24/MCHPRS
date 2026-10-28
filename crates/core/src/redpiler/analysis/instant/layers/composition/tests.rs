use super::*;
use crate::redpiler::analysis::instant::layers::{reduction::reduce_roots, tests::empty_graph};
use mchprs_blocks::{BlockPos, blocks::Block};

fn occurrence(actor: u32) -> RouteOccurrence {
    RouteOccurrence {
        route: RouteId::new(actor),
        ordinal: 0,
    }
}
fn cell(actor: u32, resource: u32, power: Option<PowerLink>) -> CellContract {
    let actor = PistonId::new(actor);
    let mut pins = vec![
        CellPin::Read(CellRead::State(StateTest::PistonAvailability {
            piston: actor,
            is: PistonAvailability::Ready,
        })),
        CellPin::Read(CellRead::State(StateTest::PistonExtended(actor))),
        CellPin::Read(CellRead::Activation(actor)),
        CellPin::Read(CellRead::State(StateTest::ResourceEnabled(ResourceId::new(resource)))),
        CellPin::Read(CellRead::State(StateTest::ResourceAt {
            resource: ResourceId::new(resource),
            contact: ContactId::new(0),
        })),
        CellPin::Gate {
            kind: CellGateKind::All,
            inputs: vec![CellPinId(3), CellPinId(4)].into(),
        },
    ];
    pins.push(match power {
        Some(link) => CellPin::Read(CellRead::Contribution(link)),
        None => CellPin::Gate {
            kind: CellGateKind::Any,
            inputs: Box::new([]),
        },
    });
    pins.push(CellPin::Gate {
        kind: CellGateKind::Invert,
        inputs: vec![CellPinId(6)].into(),
    });
    pins.push(CellPin::Gate {
        kind: CellGateKind::All,
        inputs: vec![CellPinId(0), CellPinId(1), CellPinId(2), CellPinId(7)].into(),
    });
    pins.push(CellPin::Gate {
        kind: CellGateKind::All,
        inputs: vec![CellPinId(0), CellPinId(1), CellPinId(7)].into(),
    });
    pins.push(CellPin::Gate {
        kind: CellGateKind::Invert,
        inputs: vec![CellPinId(5)].into(),
    });
    let mut use_ = super::super::tests::use_with_activations(vec![occurrence(actor.index() as u32)].into(), RouteId::new(0), RouteId::new(0), None);
    use_.actor = actor;
    use_.requested = CellPinId(8);
    use_.valid = CellPinId(9);
    use_.powered = CellPinId(6);
    use_.retraction.transfer = Some(CellResourceTransfer {
        resource: ResourceId::new(resource),
        source: ContactId::new(0),
        destination: ContactId::new(actor.index() as u32 + 1),
        resource_revision_increment: 1,
        when_at_source: CellPinId(5),
    });
    CellContract {
        uses: vec![use_].into(),
        pins: pins.into(),
        stationary_entry: CellPinId(0),
        resources: Box::new([CellResourcePin {
            resource: ResourceId::new(resource),
            source: ContactId::new(0),
            present: CellPinId(5),
            absent: CellPinId(10),
            owners: Box::new([actor]),
            selection: CellTokenSelection::FirstAcceptedPullRequestInFifo,
        }]),
    }
}

fn fixture(shared: bool) -> (CompileGraph, BoundaryCatalog, RegionComponent) {
    let link = PowerLink {
        from: SiteId::new(0),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: NodeId::new(0),
            port: InputPort::Main,
        },
        face: BlockFace::West,
        attenuation: 2,
        guard: GuardId::new(0),
    };
    let mut graph = empty_graph(link);
    graph.power_links.clear();
    graph.resources = (0..2)
        .map(|_| ResourceSpec {
            material: ProfileId::new(0),
            contacts: vec![ContactId::new(0), ContactId::new(1), ContactId::new(2)].into(),
        })
        .collect();
    graph.guards.push(Guard::State(StateTest::ResourceEnabled(ResourceId::new(0))));
    graph.guards.push(Guard::Not(GuardId::new(1)));
    let pos = BlockPos::new(0, 20, 0);
    graph.sites.push(SiteSpec {
        position: pos,
        source: SiteSource::Contact(ContactId::new(0)),
        source_strength: 0,
    });
    graph.profiles = [Block::RedstoneBlock, Block::Air]
        .into_iter()
        .map(|block| Profile {
            raw_state: block.get_id(),
            registry_type: 0,
            solid: block.is_solid(),
            transparent: block.is_transparent(),
            cube: block.is_cube(),
            electrical: if block == Block::RedstoneBlock {
                ProfileElectrical::RedstoneSource
            } else {
                ProfileElectrical::None
            },
        })
        .collect();
    graph.contacts.push(ContactSpec {
        node: NodeId::new(0),
        position: pos,
        variants: vec![
            ContactVariant {
                when: GuardId::new(1),
                profile: ProfileId::new(0),
                receiver: None,
            },
            ContactVariant {
                when: GuardId::new(2),
                profile: ProfileId::new(1),
                receiver: None,
            },
        ]
        .into(),
    });
    let catalog = BoundaryCatalog {
        cells: vec![
            cell(0, 0, None),
            cell(1, if shared { 0 } else { 1 }, if shared { None } else { Some(link) }),
        ]
        .into(),
        ..Default::default()
    };
    let component = RegionComponent {
        cells: vec![0, 1].into(),
        corridors: Box::new([]),
        ports: Box::new([]),
    };
    (graph, catalog, component)
}

fn evaluated(composition: &Composition<'_>, roots: &[LayerExprId]) -> Vec<bool> {
    let roots = roots.iter().enumerate().map(|(i, &root)| (LayerRoot::Output(i), root)).collect();
    let program = reduce_roots(
        &composition.builder.inputs,
        &composition.builder.expressions,
        roots,
        &TaskMonitor::default(),
    )
    .unwrap();
    let mut values = vec![false; program.expressions.len()];
    program
        .evaluate(&mut values, |input| match input.source {
            LayerInputSource::State(StateTest::PistonExtended(_) | StateTest::PistonHeadPresent(_) | StateTest::ResourceEnabled(_)) => true,
            LayerInputSource::State(StateTest::ResourceAt { contact, .. }) => contact == ContactId::new(0),
            LayerInputSource::State(StateTest::PistonAvailability { is, .. }) => is == PistonAvailability::Ready,
            other => panic!("unexpected input {other:?}"),
        })
        .unwrap();
    program.roots.iter().map(|(_, root)| values[root.0 as usize]).collect()
}

#[test]
fn combine_flattens_nested_boolean_operators_without_expanding_shared_subtrees() {
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let input = |id| LayerInputSource::BoundaryEvent {
        port: id,
        occurrence: RouteOccurrence {
            route: RouteId::new(id),
            ordinal: 0,
        },
    };
    let a_input = builder.input(input(0), LayerInputRole::ActivationEvent).unwrap();
    let a = builder.input_expr(a_input).unwrap();
    let b_input = builder.input(input(1), LayerInputRole::ActivationEvent).unwrap();
    let b = builder.input_expr(b_input).unwrap();
    let c_input = builder.input(input(2), LayerInputRole::ActivationEvent).unwrap();
    let c = builder.input_expr(c_input).unwrap();
    let not_a = builder.not(a).unwrap();
    let nested_all = builder.all([b, not_a]).unwrap();
    let nested_any = builder.any([b, a]).unwrap();

    let contradiction = builder.all([a, nested_all]).unwrap();
    let tautology = builder.any([not_a, nested_any]).unwrap();
    assert_eq!(builder.expression(contradiction).unwrap(), &LayerExpr::Constant(false));
    assert_eq!(builder.expression(tautology).unwrap(), &LayerExpr::Constant(true));

    let shared = builder.all([a, b]).unwrap();
    let through_shared = builder.all([shared, c, shared]).unwrap();
    let flat = builder.all([c, a, b]).unwrap();
    assert_eq!(through_shared, flat, "flattening preserves hash-consed shared operands");
    assert_eq!(builder.expression(flat).unwrap(), &LayerExpr::All(vec![a, b, c].into()));
}

#[test]
fn flattened_boolean_walk_keeps_cancellation_and_work_limits() {
    let monitor = TaskMonitor::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let input = builder
        .input(
            LayerInputSource::BoundaryEvent {
                port: 0,
                occurrence: occurrence(0),
            },
            LayerInputRole::ActivationEvent,
        )
        .unwrap();
    let a = builder.input_expr(input).unwrap();
    let yes = builder.constant(true).unwrap();
    let nested = builder.all([a, yes]).unwrap();
    builder.budget.work = MAX_LAYER_WORK;
    assert_eq!(builder.all([nested]), Err(ConstructionError::BudgetExceeded));

    let cancelled = TaskMonitor::default();
    let mut cancelled_builder = LayerBuilder::new(&cancelled).unwrap();
    let input = cancelled_builder.constant(true).unwrap();
    cancelled.cancel();
    assert!(cancelled_builder.all([input]).is_err());
}

#[test]
fn queued_entry_preserves_the_sampled_request_and_revalidates_live_power() {
    let (graph, catalog, component) = fixture(false);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut composition = Composition::new(&graph, &catalog, &power, &component, &monitor).unwrap();
    let yes = composition.builder.constant(true).unwrap();
    composition.queued_sample(1, PistonId::new(1), yes, occurrence(1)).unwrap();
    let acceptance = composition.begin_next().unwrap().unwrap();
    composition.finish_base(acceptance).unwrap();
    let use_ = &composition.layers[0].uses[0];
    assert_eq!(evaluated(&composition, &[use_.requested, use_.accepted]), [true, false]);

    let no = composition.builder.constant(false).unwrap();
    composition.queued_sample(0, PistonId::new(0), no, occurrence(0)).unwrap();
    assert!(composition.begin_next().unwrap().is_none());
    assert_eq!(composition.layers.len(), 1, "a statically false queued request creates no causal layer");
}

#[test]
fn partial_symbolic_wave_is_discarded_when_its_region_exhausts_the_proof_budget() {
    let (graph, catalog, component) = fixture(true);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut composition = Composition::new(&graph, &catalog, &power, &component, &monitor).unwrap();
    let yes = composition.builder.constant(true).unwrap();
    composition.sample(0, PistonId::new(0), yes, vec![occurrence(0)].into()).unwrap();
    let acceptance = composition.begin_next().unwrap().unwrap();
    composition.finish_base(acceptance).unwrap();
    assert_eq!(composition.resources.len(), 1);

    composition.builder.budget.work = MAX_LAYER_WORK;
    let error = composition.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap_err();
    assert_eq!(error, ConstructionError::BudgetExceeded);
    let decline = component_result(&graph, &catalog, &component, Err(error)).unwrap().err().unwrap();
    assert_eq!(decline.cells, component.cells);
    assert_eq!(decline.corridors, component.corridors);
    assert_eq!(decline.reason, LayerDeclineReason::ProofBudgetExceeded);
    assert_eq!(decline.witness, LayerWitness::Actor(PistonId::new(0)));
    assert!(graph.plans.instant.layers.is_none());
}

#[test]
fn failed_samples_can_retry_after_a_causal_resource_change_without_burning_the_use() {
    let (graph, catalog, component) = fixture(false);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut composition = Composition::new(&graph, &catalog, &power, &component, &monitor).unwrap();
    let yes = composition.builder.constant(true).unwrap();
    composition.sample(0, PistonId::new(0), yes, vec![occurrence(0)].into()).unwrap();
    composition.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    let first = composition.begin_next().unwrap().unwrap();
    // The base phase still observes the old source; Far is the first cut with its absence.
    composition.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    assert!(composition.begin_next().is_err());
    composition.finish_base(first).unwrap();
    composition.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    while let Some(next) = composition.begin_next().unwrap() {
        composition.finish_base(next).unwrap();
    }
    let accepted: Vec<_> = composition.layers.iter().map(|layer| layer.uses[0].accepted).collect();
    assert_eq!(
        evaluated(&composition, &accepted),
        vec![true, false, true],
        "the statically false retry is omitted; the later live retry still gets its turn"
    );
    assert_eq!(
        evaluated(&composition, &composition.resources.iter().map(|r| r.transfer_when).collect::<Vec<_>>()),
        vec![true, false, true]
    );
}

#[test]
fn shared_resource_has_one_fifo_winner_and_reversing_requests_changes_the_destination() {
    for order in [[0, 1], [1, 0]] {
        let (graph, catalog, component) = fixture(true);
        let monitor = TaskMonitor::default();
        let power = index_power(&graph);
        let mut composition = Composition::new(&graph, &catalog, &power, &component, &monitor).unwrap();
        let yes = composition.builder.constant(true).unwrap();
        for actor in order {
            composition
                .sample(actor, PistonId::new(actor), yes, vec![occurrence(actor)].into())
                .unwrap();
        }
        while let Some(next) = composition.begin_next().unwrap() {
            composition.finish_base(next).unwrap();
        }
        assert_eq!(
            evaluated(
                &composition,
                &composition.layers.iter().map(|layer| layer.uses[0].accepted).collect::<Vec<_>>()
            ),
            vec![true, true]
        );
        assert_eq!(
            evaluated(&composition, &composition.resources.iter().map(|r| r.transfer_when).collect::<Vec<_>>()),
            vec![true, false]
        );
        assert_eq!(composition.resources[0].destination, ContactId::new(order[0] + 1));
    }
}

#[test]
fn queued_duplicates_and_feedback_cannot_accept_an_actor_twice() {
    let (graph, catalog, component) = fixture(true);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut composition = Composition::new(&graph, &catalog, &power, &component, &monitor).unwrap();
    let no = composition.builder.constant(false).unwrap();
    let yes = composition.builder.constant(true).unwrap();
    for event in [no, yes, yes] {
        composition.sample(0, PistonId::new(0), event, vec![occurrence(0)].into()).unwrap();
    }
    assert_eq!(
        composition.requests.len(),
        1,
        "the false request and an already-pending duplicate are pruned"
    );
    while let Some(next) = composition.begin_next().unwrap() {
        composition.finish_base(next).unwrap();
    }
    composition.sample(0, PistonId::new(0), yes, vec![occurrence(0)].into()).unwrap();
    let next = composition.begin_next().unwrap().unwrap();
    composition.finish_base(next).unwrap();
    assert_eq!(
        evaluated(
            &composition,
            &composition.layers.iter().map(|layer| layer.uses[0].accepted).collect::<Vec<_>>()
        ),
        vec![true, false]
    );
}

#[test]
fn resource_wave_consumes_shared_block_once_and_rearms_next_cycle() {
    let (graph, catalog, component) = fixture(true);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut wave = Composition::new_resource_wave(&graph, &catalog, &power, &component, &monitor).unwrap();
    let yes = wave.builder.constant(true).unwrap();
    wave.sample(0, PistonId::new(0), yes, vec![occurrence(0)].into()).unwrap();
    let first = wave.begin_next().unwrap().unwrap();
    wave.finish_base(first).unwrap();
    wave.write(StateTest::ResourceEnabled(ResourceId::new(0)), true, yes).unwrap();
    wave.write(
        StateTest::ResourceAt {
            resource: ResourceId::new(0),
            contact: ContactId::new(0),
        },
        true,
        yes,
    )
    .unwrap();
    wave.write(
        StateTest::ResourceAt {
            resource: ResourceId::new(0),
            contact: ContactId::new(1),
        },
        false,
        yes,
    )
    .unwrap();
    wave.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    let second = wave.begin_next().unwrap().unwrap();
    wave.finish_base(second).unwrap();
    assert_eq!(
        evaluated(&wave, &wave.layers.iter().map(|layer| layer.uses[0].accepted).collect::<Vec<_>>()),
        [true, false]
    );
    assert_eq!(
        evaluated(&wave, &wave.resources.iter().map(|output| output.transfer_when).collect::<Vec<_>>()),
        [true, false]
    );

    let mut next = Composition::new_resource_wave(&graph, &catalog, &power, &component, &monitor).unwrap();
    let yes = next.builder.constant(true).unwrap();
    next.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    let accepted = next.begin_next().unwrap().unwrap();
    next.finish_base(accepted).unwrap();
    assert_eq!(evaluated(&next, &[next.layers[0].uses[0].accepted]), [true]);

    let mut unsupported = catalog.clone();
    unsupported.cells[0].uses[0].retraction.transfer = None;
    assert!(Composition::new_resource_wave(&graph, &unsupported, &power, &component, &monitor).is_err());
    let mut ambiguous = catalog.clone();
    ambiguous.cells[1].uses[0].actor = PistonId::new(0);
    assert!(Composition::new_resource_wave(&graph, &ambiguous, &power, &component, &monitor).is_err());
    let mut multiple_sources = catalog.clone();
    let mut resources = multiple_sources.cells[0].resources.to_vec();
    let mut second = resources[0].clone();
    second.resource = ResourceId::new(1);
    resources.push(second);
    multiple_sources.cells[0].resources = resources.into_boxed_slice();
    assert!(Composition::new_resource_wave(&graph, &multiple_sources, &power, &component, &monitor).is_err());
    let mut invalid = catalog.clone();
    invalid.cells[0].uses[0].retraction.transfer.as_mut().unwrap().destination = ContactId::new(99);
    assert!(Composition::new_resource_wave(&graph, &invalid, &power, &component, &monitor).is_err());
}

#[test]
fn resource_wave_needs_activation_and_failed_request_does_not_consume() {
    let (graph, catalog, component) = fixture(true);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut wave = Composition::new_resource_wave(&graph, &catalog, &power, &component, &monitor).unwrap();
    let no = wave.builder.constant(false).unwrap();
    let yes = wave.builder.constant(true).unwrap();

    wave.sample(0, PistonId::new(0), no, vec![occurrence(0)].into()).unwrap();
    assert!(wave.begin_next().unwrap().is_none(), "an unpowered actor needs a qualifying event");
    assert!(wave.resource_consumed.as_ref().unwrap().is_empty());

    wave.write(StateTest::PistonExtended(PistonId::new(0)), false, yes).unwrap();
    wave.queued_sample(0, PistonId::new(0), yes, occurrence(0)).unwrap();
    let failed = wave.begin_next().unwrap().unwrap();
    wave.finish_base(failed).unwrap();
    wave.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    let successful = wave.begin_next().unwrap().unwrap();
    wave.finish_base(successful).unwrap();

    assert_eq!(
        evaluated(&wave, &wave.layers.iter().map(|layer| layer.uses[0].accepted).collect::<Vec<_>>()),
        [false, true]
    );
    assert_eq!(
        evaluated(&wave, &wave.resources.iter().map(|output| output.transfer_when).collect::<Vec<_>>()),
        [false, true]
    );
}

#[test]
fn resource_wave_missing_block_does_not_consume_it() {
    let (graph, catalog, component) = fixture(true);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut wave = Composition::new_resource_wave(&graph, &catalog, &power, &component, &monitor).unwrap();
    let yes = wave.builder.constant(true).unwrap();
    wave.write(StateTest::ResourceEnabled(ResourceId::new(0)), false, yes).unwrap();
    wave.queued_sample(0, PistonId::new(0), yes, occurrence(0)).unwrap();
    let failed = wave.begin_next().unwrap().unwrap();
    wave.finish_base(failed).unwrap();
    wave.write(StateTest::ResourceEnabled(ResourceId::new(0)), true, yes).unwrap();
    wave.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    let successful = wave.begin_next().unwrap().unwrap();
    wave.finish_base(successful).unwrap();
    assert_eq!(
        evaluated(&wave, &wave.layers.iter().map(|layer| layer.uses[0].accepted).collect::<Vec<_>>()),
        [false, true]
    );
    assert_eq!(
        evaluated(&wave, &wave.resources.iter().map(|output| output.transfer_when).collect::<Vec<_>>()),
        [false, true]
    );
}

#[test]
fn resource_wave_allows_different_blocks_in_one_cycle() {
    let (graph, catalog, component) = fixture(false);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut wave = Composition::new_resource_wave(&graph, &catalog, &power, &component, &monitor).unwrap();
    let yes = wave.builder.constant(true).unwrap();
    wave.write(StateTest::ResourceEnabled(ResourceId::new(1)), false, yes).unwrap();
    wave.sample(0, PistonId::new(0), yes, vec![occurrence(0)].into()).unwrap();
    let first = wave.begin_next().unwrap().unwrap();
    wave.finish_base(first).unwrap();
    wave.write(StateTest::ResourceEnabled(ResourceId::new(1)), true, yes).unwrap();
    wave.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    let second = wave.begin_next().unwrap().unwrap();
    wave.finish_base(second).unwrap();
    assert_eq!(
        evaluated(&wave, &wave.resources.iter().map(|output| output.transfer_when).collect::<Vec<_>>()),
        [true, true]
    );
}
