//! Equation checks for graph-derived stationary pull cells.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::instant::cells::{CellDeclineReason, discover_cells, validate_cells};
use crate::redpiler::backend::graph::GraphObservedOperation;
use std::collections::BTreeSet;

fn state_matches(backend: &GraphBackend, test: StateTest) -> bool {
    match test {
        StateTest::PistonExtended(piston) => backend.state.pistons[piston.index()].extended,
        StateTest::PistonHeadPresent(piston) => backend.state.pistons[piston.index()].head_present,
        StateTest::PistonAvailability { piston, is } => backend.state.pistons[piston.index()].availability == is,
        StateTest::ResourceAt { resource, contact } => backend.state.resources[resource.index()].at == contact,
        StateTest::ResourceEnabled(resource) => backend.state.resources[resource.index()].enabled,
    }
}

fn evaluate(cell: &CellContract, backend: &GraphBackend, active: &BTreeSet<PistonId>) -> Vec<bool> {
    let mut values = Vec::new();
    cell.evaluate(&mut values, |read| match read {
        CellRead::Contribution(link) => {
            backend.guard_matches(link.guard, None) && backend.site_power(link.from, link.from_port).saturating_sub(link.attenuation) > 0
        }
        CellRead::Activation(actor) => active.contains(&actor),
        CellRead::State(test) => state_matches(backend, test),
    })
    .expect("well-formed cell equations");
    values
}

fn assert_matches_graph(cell: &CellContract, backend: &GraphBackend, active: &BTreeSet<PistonId>) {
    let values = evaluate(cell, backend, active);
    let entry_ready = cell.uses.iter().all(|use_| {
        let state = backend.state.pistons[use_.actor.index()];
        state.availability == PistonAvailability::Ready && state.extended
    });
    let resources_ready = cell.resources.iter().all(|resource| {
        let state = backend.state.resources[resource.resource.index()];
        state.at == resource.source && state.enabled
    });
    assert_eq!(values[cell.stationary_entry.index()], entry_ready && resources_ready);

    for use_ in cell.uses.iter() {
        let spec = backend.program.pistons()[use_.actor.index()];
        let powered = backend.input_strength(spec.node, InputPort::Main) > 0 || backend.input_strength(spec.node, InputPort::Quasi) > 0;
        assert_eq!(values[use_.powered.index()], powered, "typed contribution equations for {:?}", use_.actor);
        let state = backend.state.pistons[use_.actor.index()];
        let requested = active.contains(&use_.actor) && state.availability == PistonAvailability::Ready && state.extended && !powered;
        assert_eq!(
            values[use_.requested.index()],
            requested,
            "activation and sampled power for {:?}",
            use_.actor
        );
        let valid =
            state.availability == PistonAvailability::Ready && state.extended && !powered && backend.piston_retraction_available(use_.actor, false);
        assert_eq!(
            values[use_.valid.index()],
            valid,
            "native pull-availability predicate for {:?}",
            use_.actor
        );
    }
}

fn set_source_strength(backend: &mut GraphBackend, node: NodeId, strength: u8) -> bool {
    match &mut backend.state.components[node.index()] {
        ComponentState::Electrical(state) => {
            state.output_strength = strength;
            true
        }
        ComponentState::Wire(wire) => {
            wire.power = strength;
            true
        }
        ComponentState::Specialized => false,
    }
}

fn check_admitted_inputs(graph: &CompileGraph, cells: &[CellContract]) -> usize {
    let (program, state) = graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    let active: BTreeSet<_> = cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect();
    let mut checks = 0;

    for cell in cells {
        for pin in cell.pins.iter() {
            let CellPin::Read(read) = pin else { continue };
            match read {
                CellRead::Contribution(link) => assert!(graph.power_links.contains(link), "power inputs retain original typed links"),
                CellRead::Activation(actor) => assert!(active.contains(actor), "activation is a graph piston event"),
                CellRead::State(
                    StateTest::PistonExtended(_)
                    | StateTest::PistonHeadPresent(_)
                    | StateTest::PistonAvailability { .. }
                    | StateTest::ResourceAt { .. }
                    | StateTest::ResourceEnabled(_),
                ) => {}
            }
        }
        assert_matches_graph(cell, &backend, &active);
        checks += 1;

        let mut source_nodes = BTreeSet::new();
        for pin in cell.pins.iter() {
            let CellPin::Read(CellRead::Contribution(link)) = pin else { continue };
            if let SiteSource::Component(node) = graph.sites[link.from.index()].source {
                source_nodes.insert(node);
            }
        }
        for node in source_nodes {
            let before = backend.state.components[node.index()].clone();
            for strength in 0..=15 {
                if set_source_strength(&mut backend, node, strength) {
                    assert_matches_graph(cell, &backend, &active);
                    checks += 1;
                }
            }
            backend.state.components[node.index()] = before;
        }

        for use_ in cell.uses.iter() {
            let before = backend.state.pistons[use_.actor.index()];
            for availability in [
                PistonAvailability::Ready,
                PistonAvailability::HeadPending,
                PistonAvailability::PublishingHeadRemoval,
                PistonAvailability::BasePending,
                PistonAvailability::BasePendingHeadPresent,
            ] {
                for extended in [false, true] {
                    for head_present in [false, true] {
                        let state = &mut backend.state.pistons[use_.actor.index()];
                        state.availability = availability;
                        state.extended = extended;
                        state.head_present = head_present;
                        assert_matches_graph(cell, &backend, &active);
                        checks += 1;
                    }
                }
            }
            backend.state.pistons[use_.actor.index()] = before;
        }

        for resource in cell.resources.iter() {
            let before = backend.state.resources[resource.resource.index()];
            let contacts = graph.resources[resource.resource.index()].contacts.clone();
            for contact in contacts.iter().copied() {
                for enabled in [false, true] {
                    let state = &mut backend.state.resources[resource.resource.index()];
                    state.at = contact;
                    state.enabled = enabled;
                    assert_matches_graph(cell, &backend, &active);
                    checks += 1;
                }
            }
            backend.state.resources[resource.resource.index()] = before;
        }
    }
    checks
}

fn expected_reset_rows(graph: &CompileGraph, route: RouteId, reset: NodeId, facing: BlockFace) -> Vec<RouteOccurrence> {
    graph.routes[route.index()]
        .deliveries
        .iter()
        .enumerate()
        .filter_map(|(ordinal, delivery)| {
            (delivery.receiver == Some(CallbackReceiver::Component(reset))
                && delivery.kind == (DeliveryKind::Update { direction: Some(facing) })
                && delivery.view == OperandView::Live
                && matches!(graph.guards[delivery.guard.index()], Guard::Always))
            .then_some(RouteOccurrence {
                route,
                ordinal: ordinal as u32,
            })
        })
        .collect()
}

fn assert_retraction_effects_match_graph(graph: &CompileGraph, cells: &[CellContract]) -> usize {
    let mut uses = 0;
    for cell in cells {
        for resource in &cell.resources {
            let expected_owners: Vec<_> = cell
                .uses
                .iter()
                .filter_map(|use_| {
                    let spec = graph.pistons[use_.actor.index()];
                    (matches!(spec.lane, PistonLane::SingleResource { far } if far == resource.source)
                        && graph.resources[resource.resource.index()].contacts.contains(&spec.near))
                    .then_some(use_.actor)
                })
                .collect();
            assert_eq!(resource.owners.as_ref(), expected_owners.as_slice());
            assert_eq!(resource.selection, CellTokenSelection::FirstAcceptedPullRequestInFifo);
        }

        for use_ in &cell.uses {
            let spec = graph.pistons[use_.actor.index()];
            let effect = &use_.retraction;
            assert_eq!(effect.routes.base_changed, spec.routes.base_changed);
            assert_eq!(effect.routes.near_changed, spec.routes.near_changed);
            let expected_transfer = cell.resources.iter().find_map(|resource| {
                resource.owners.contains(&use_.actor).then_some(CellResourceTransfer {
                    resource: resource.resource,
                    source: resource.source,
                    destination: spec.near,
                    resource_revision_increment: 1,
                    when_at_source: resource.present,
                })
            });
            assert_eq!(effect.transfer, expected_transfer);
            assert_eq!(
                effect.routes.far_changed,
                expected_transfer.and(spec.routes.far_changed),
                "Far callbacks follow an actual token transfer"
            );
            assert_eq!(effect.wake.ticks_after_acceptance, 2);
            assert_eq!(effect.actor_revision_increment, 1);
            assert_eq!(effect.wake.base_restored, spec.routes.base_restored);
            assert_eq!(effect.wake.resource.is_some(), expected_transfer.is_some());
            if let (Some(transfer), Some(wake)) = (expected_transfer, effect.wake.resource) {
                assert_eq!(wake.resource, transfer.resource);
                assert_eq!(wake.contact, transfer.destination);
                assert_eq!(wake.route, spec.routes.resource_restored_near.unwrap());
            }

            let watched = graph.pistons[effect.reset.watched_actor.index()];
            let facing = match graph.nodes[use_.reset.index()].kind {
                CircuitNodeKind::Observer { facing, .. } | CircuitNodeKind::BuiltinReset { facing, .. } => facing,
                _ => panic!("reset provenance points at an observer or built-in reset"),
            };
            assert_eq!(
                effect.reset.on_base_changed.as_ref(),
                expected_reset_rows(graph, watched.routes.base_changed, use_.reset, facing).as_slice()
            );
            assert_eq!(
                effect.reset.on_base_restored.as_ref(),
                expected_reset_rows(graph, watched.routes.base_restored, use_.reset, facing).as_slice()
            );
            assert!(!effect.reset.on_base_changed.is_empty());
            assert!(!effect.reset.on_base_restored.is_empty());
            uses += 1;
        }
    }
    uses
}

fn prepared_fixture_cells(id: &'static str) -> (CompileGraph, crate::redpiler::analysis::instant::cells::CellDiscovery) {
    let circuit = boundary_contract_tests::load_mechanism(id);
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
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
    let discovery = discover_cells(&prepared.graph, &monitor).unwrap();
    (prepared.graph, discovery)
}

fn assert_all_fixture_actors_accounted(id: &'static str, expected: usize) -> (CompileGraph, Vec<CellContract>) {
    let (graph, _) = stable_region_tests::stable_graph(id);
    assert_eq!(graph.pistons.len(), expected);
    let discovery = discover_cells(&graph, &TaskMonitor::default()).unwrap();
    assert!(
        discovery.declined.is_empty(),
        "all {id} actors have complete stationary-cell contracts: {:?}",
        discovery.declined
    );
    let mut accounted = BTreeSet::new();
    for cell in &discovery.cells {
        for use_ in cell.uses.iter() {
            assert!(accounted.insert(use_.actor), "actor {:?} has more than one local cell owner", use_.actor);
        }
    }
    for decline in &discovery.declined {
        for &actor in &decline.actors {
            assert!(accounted.insert(actor), "declined actor {actor:?} is also admitted elsewhere");
        }
    }
    assert_eq!(accounted.len(), expected, "every loaded fixture actor is admitted or explicitly declined");
    assert!(
        discovery.declined.is_empty(),
        "every {id} actor has a complete stationary cell: {:?}",
        discovery.declined
    );
    assert_eq!(
        discovery.cells.iter().map(|cell| cell.uses.len()).sum::<usize>(),
        expected,
        "all {id} actors are admitted"
    );
    (graph, discovery.cells)
}

fn change_reset_direction(graph: &mut CompileGraph, observer: NodeId, direction: BlockFace) {
    graph.nodes[observer.index()].kind = match graph.nodes[observer.index()].kind {
        CircuitNodeKind::Observer { watched, .. } => CircuitNodeKind::Observer { watched, facing: direction },
        CircuitNodeKind::BuiltinReset { watched, .. } => CircuitNodeKind::BuiltinReset { watched, facing: direction },
        _ => panic!("reset provenance points to an observer node"),
    };
}

fn erase_reset_recognition(graph: &mut CompileGraph, observer: NodeId) {
    graph.nodes[observer.index()].kind = CircuitNodeKind::Electrical(NodeType::Torch);
}

#[test]
fn physical_adder_preparation_reaches_the_stationary_first_response_entry() {
    for (id, actors) in [("adder_1bit", 13), ("adder_11bits", 142)] {
        let circuit = load_adder(id, ORIGIN, None);
        let (graph, cells) = assert_all_fixture_actors_accounted(id, actors);
        let cases = super::instant_contract_tests::arithmetic_cases(id);
        for case in &cases {
            let mut native = circuit.build();
            for step in case_steps(&circuit, case, false, false) {
                native_step(&mut native, step);
            }
            // A source-derived stationary contract must be reachable after the actual
            // physical data preparation, not merely true in the saved schematic.
            for cell in &cells {
                for use_ in &cell.uses {
                    let spec = graph.pistons[use_.actor.index()];
                    let position = spec.position;
                    assert!(
                        matches!(native.get_block(position), Block::Piston { piston } if piston.extended),
                        "{id} {}: actor {:?} has not returned extended before activation",
                        case["id"],
                        use_.actor
                    );
                    let head = graph.contacts[spec.near.index()].position;
                    assert!(
                        matches!(native.get_block(head), Block::PistonHead { .. }),
                        "{id} {}: actor {:?} still has a moving/missing head before activation",
                        case["id"],
                        use_.actor
                    );
                }
                for output in &cell.resources {
                    let spec = &graph.resources[output.resource.index()];
                    let source = graph.contacts[output.source.index()].position;
                    let material = Block::from_id(graph.profiles[spec.material.index()].raw_state);
                    assert_eq!(
                        native.get_block(source),
                        material,
                        "{id} {}: resource {:?} is not stationary at the admitted source",
                        case["id"],
                        output.resource
                    );
                }
            }
        }
        println!(
            "{id}: {} prepared native histories reach all stationary first-response cells",
            cases.len()
        );
    }
}

#[test]
fn both_adders_lower_typed_stationary_cells_and_match_graph_queries() {
    for (id, actors) in [("adder_1bit", 13), ("adder_11bits", 142)] {
        let (graph, cells) = assert_all_fixture_actors_accounted(id, actors);
        assert!(!cells.is_empty(), "{id} has stationary pull cells");
        assert_eq!(assert_retraction_effects_match_graph(&graph, &cells), actors);
        let checks = check_admitted_inputs(&graph, &cells);
        assert!(checks > actors, "{id} exercises strength, guard and contact-state variants");
        println!("{id}: cells={}, actors={actors}, typed Graph comparisons={checks}", cells.len());
    }
}

#[test]
fn accepted_retractions_keep_shared_payload_and_existing_reset_obligations() {
    let (graph, cells) = assert_all_fixture_actors_accounted("adder_1bit", 13);
    assert_eq!(assert_retraction_effects_match_graph(&graph, &cells), 13);
    assert!(
        cells.iter().flat_map(|cell| &cell.resources).any(|resource| resource.owners.len() == 1),
        "the fixture contains a private redstone token"
    );
    let shared = cells
        .iter()
        .flat_map(|cell| &cell.resources)
        .find(|resource| resource.owners.len() > 1)
        .expect("the fixture contains a shared redstone token");
    let cell = cells
        .iter()
        .find(|cell| cell.resources.iter().any(|resource| resource.resource == shared.resource))
        .unwrap();
    let use_ = cell.uses.iter().find(|use_| use_.actor == shared.owners[0]).unwrap();
    let transfer = use_.retraction.transfer.expect("the selected owner has a conditional transfer");
    let mut values = Vec::new();
    cell.evaluate(&mut values, |read| match read {
        CellRead::Contribution(_) => false,
        CellRead::Activation(actor) => actor == use_.actor,
        CellRead::State(StateTest::PistonAvailability { piston, is }) => {
            if piston == use_.actor {
                is == PistonAvailability::Ready
            } else {
                is == PistonAvailability::BasePending
            }
        }
        CellRead::State(StateTest::PistonExtended(piston)) => piston == use_.actor,
        CellRead::State(StateTest::PistonHeadPresent(_)) => false,
        CellRead::State(StateTest::ResourceAt { .. } | StateTest::ResourceEnabled(_)) => false,
    })
    .expect("a shared token may be absent while a stationary actor still accepts a normal retract");
    assert!(values[use_.requested.index()]);
    assert!(values[use_.valid.index()]);
    assert!(
        !values[transfer.when_at_source.index()],
        "the request remains valid without a token transfer"
    );

    for id in ["or_1", "and_1", "not_1", "instant_reset_redstone"] {
        let (graph, discovery) = prepared_fixture_cells(id);
        let uses = assert_retraction_effects_match_graph(&graph, &discovery.cells);
        if id == "instant_reset_redstone" {
            let has_reset_observer = graph
                .nodes
                .iter()
                .any(|node| matches!(&node.kind, CircuitNodeKind::Observer { .. } | CircuitNodeKind::BuiltinReset { .. }));
            assert!(!has_reset_observer, "this baseline has no observer-backed reset source");
            assert!(discovery.cells.is_empty() && discovery.declined.is_empty());
        }
        println!("{id}: stationary pull uses={uses}, structured declines={:?}", discovery.declined);
    }
}

fn graph_after_shared_pull_requests(graph: &CompileGraph, resource: &CellResourcePin, owners: &[PistonId], fifo: &[PistonId]) -> GraphBackend {
    let (program, state) = graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    backend.set_test_recording(true);
    for component in &mut backend.state.components {
        match component {
            ComponentState::Electrical(state) => {
                state.powered = false;
                state.output_strength = 0;
            }
            ComponentState::Wire(wire) => wire.power = 0,
            ComponentState::Specialized => {}
        }
    }
    for state in &mut backend.state.pistons {
        *state = PistonState {
            extended: false,
            head_present: false,
            availability: PistonAvailability::BasePending,
            revision: 0,
        };
    }
    for &actor in owners {
        backend.state.pistons[actor.index()] = PistonState {
            extended: true,
            head_present: true,
            availability: PistonAvailability::Ready,
            revision: 0,
        };
    }

    let owner_near: BTreeSet<_> = owners.iter().map(|actor| graph.pistons[actor.index()].near).collect();
    let mut unavailable = owner_near.clone();
    unavailable.insert(resource.source);
    for (index, spec) in graph.resources.iter().enumerate() {
        let resource_id = ResourceId::new(index as u32);
        if resource_id == resource.resource {
            continue;
        }
        let contact = spec
            .contacts
            .iter()
            .copied()
            .find(|contact| !unavailable.contains(contact))
            .expect("each fixture resource has an unoccupied contact for the isolated action test");
        unavailable.insert(contact);
        backend.state.resources[resource_id.index()] = ResourceState {
            at: contact,
            enabled: false,
            revision: 0,
        };
    }
    backend.state.resources[resource.resource.index()] = ResourceState {
        at: resource.source,
        enabled: true,
        revision: 0,
    };

    for &actor in fifo {
        let spec = backend.program.pistons()[actor.index()];
        assert_eq!(backend.input_strength(spec.node, InputPort::Main), 0, "test input starts low");
        assert_eq!(backend.input_strength(spec.node, InputPort::Quasi), 0, "test quasi input starts low");
        backend.enqueue_piston(PistonRequest {
            piston: actor,
            action: mchprs_world::PistonAction::Retract,
        });
    }
    backend.drain_piston_actions().unwrap();
    backend
}

#[test]
fn graph_fifo_transfers_one_shared_token_and_keeps_independent_deferred_witnesses() {
    let (graph, cells) = assert_all_fixture_actors_accounted("adder_1bit", 13);
    let shared = cells
        .iter()
        .flat_map(|cell| &cell.resources)
        .find(|resource| resource.owners.len() > 1)
        .expect("the fixture contains a shared payload");
    let source = shared.source;
    let owners = [shared.owners[0], shared.owners[1]];
    let owner_near: [ContactId; 2] = owners.map(|actor| graph.pistons[actor.index()].near);
    assert_ne!(owner_near[0], owner_near[1]);
    assert!(owners.iter().all(|actor| {
        let spec = graph.pistons[actor.index()];
        matches!(spec.lane, PistonLane::SingleResource { far } if far == source)
    }));

    let shared_id = shared.resource;
    // Reverse the stable owner order so this proves queue order chooses the token recipient.
    let fifo = [owners[1], owners[0]];
    let backend = graph_after_shared_pull_requests(&graph, shared, &owners, &fifo);
    let queued: Vec<_> = backend.future.piston_requests.iter().map(|request| request.piston).collect();
    assert!(queued.is_empty(), "normal action draining consumes the FIFO");
    let requested: Vec<_> = backend
        .observed_operations
        .iter()
        .filter_map(|operation| match operation {
            GraphObservedOperation::Request { actor, .. } => Some(*actor),
            _ => None,
        })
        .take(2)
        .collect();
    let expected_positions = fifo.map(|actor| graph.nodes[graph.pistons[actor.index()].node.index()].position.unwrap());
    assert_eq!(requested.as_slice(), expected_positions.as_slice(), "requests retain Graph FIFO order");
    let accepted: Vec<_> = backend
        .observed_operations
        .iter()
        .filter_map(|operation| match operation {
            GraphObservedOperation::Accepted { actor, .. } => Some(*actor),
            _ => None,
        })
        .take(2)
        .collect();
    assert_eq!(
        accepted.as_slice(),
        expected_positions.as_slice(),
        "both stationary pull requests are accepted in FIFO order"
    );

    let events: Vec<_> = backend.future.deferred_pistons.iter().take(2).copied().collect();
    assert_eq!(events.len(), 2, "both FIFO requests produce independent deferred actor events");
    assert_eq!(events[0].piston, fifo[0]);
    assert_eq!(events[1].piston, fifo[1]);
    assert_eq!(events[0].due_tick, 2);
    assert_eq!(events[1].due_tick, 2);
    assert!(!events[0].extending);
    assert!(!events[1].extending);
    assert_eq!(events[0].revision, 1);
    assert_eq!(events[1].revision, 1);
    assert_eq!(
        events[0].resource.map(|event| (event.resource, event.revision, event.contact)),
        Some((shared_id, 1, owner_near[1]))
    );
    assert!(
        events[1].resource.is_none(),
        "the later accepted owner cannot transfer the already-moved token"
    );
    let resource = backend.state.resources[shared_id.index()];
    assert_eq!(
        resource,
        ResourceState {
            at: owner_near[1],
            enabled: false,
            revision: 1
        }
    );

    let mut valid = graph_after_shared_pull_requests(&graph, shared, &owners, &[fifo[0]]);
    let event = valid.future.deferred_pistons.front().copied().unwrap();
    valid.apply_deferred_event(event).unwrap();
    assert_eq!(valid.state.pistons[fifo[0].index()].availability, PistonAvailability::Ready);
    assert!(!valid.state.pistons[fifo[0].index()].extended);
    assert_eq!(valid.state.resources[shared_id.index()].at, owner_near[1]);
    assert!(valid.state.resources[shared_id.index()].enabled);
    assert_eq!(valid.state.resources[shared_id.index()].revision, 2);

    let mut stale_actor = graph_after_shared_pull_requests(&graph, shared, &owners, &[fifo[0]]);
    let event = stale_actor.future.deferred_pistons.front().copied().unwrap();
    stale_actor.state.pistons[fifo[0].index()].revision = event.revision + 1;
    stale_actor.apply_deferred_event(event).unwrap();
    assert_eq!(stale_actor.state.pistons[fifo[0].index()].availability, PistonAvailability::BasePending);
    assert!(stale_actor.state.pistons[fifo[0].index()].extended);
    assert_eq!(stale_actor.state.resources[shared_id.index()].at, owner_near[1]);
    assert!(stale_actor.state.resources[shared_id.index()].enabled);
    assert_eq!(stale_actor.state.resources[shared_id.index()].revision, 2);

    let mut stale_resource = graph_after_shared_pull_requests(&graph, shared, &owners, &[fifo[0]]);
    let event = stale_resource.future.deferred_pistons.front().copied().unwrap();
    stale_resource
        .set_resource_contact(shared_id, source, false)
        .expect("a later resource move invalidates the captured resource revision");
    stale_resource.apply_deferred_event(event).unwrap();
    assert_eq!(stale_resource.state.pistons[fifo[0].index()].availability, PistonAvailability::Ready);
    assert!(!stale_resource.state.pistons[fifo[0].index()].extended);
    assert_eq!(stale_resource.state.resources[shared_id.index()].at, source);
    assert!(!stale_resource.state.resources[shared_id.index()].enabled);
    assert_eq!(stale_resource.state.resources[shared_id.index()].revision, 2);
}

#[test]
fn graph_witness_validation_rejects_omitted_guard_wrong_polarity_and_missing_resource() {
    let (graph, cells) = assert_all_fixture_actors_accounted("adder_1bit", 13);
    let monitor = TaskMonitor::default();
    validate_cells(&graph, &cells, &monitor).expect("complete cell table matches graph-derived witnesses");

    let mut omitted_guard = cells.clone();
    let cell = omitted_guard
        .iter_mut()
        .find(|cell| {
            cell.pins
                .iter()
                .any(|pin| matches!(pin, CellPin::Read(CellRead::Contribution(link)) if link.guard.index() != 0))
        })
        .unwrap();
    let pins = cell.pins.to_vec();
    let index = pins
        .iter()
        .position(|pin| matches!(pin, CellPin::Read(CellRead::Contribution(link)) if link.guard.index() != 0))
        .unwrap();
    let mut pins = pins;
    let CellPin::Read(CellRead::Contribution(mut link)) = pins[index] else {
        unreachable!()
    };
    link.guard = GuardId::new(0);
    pins[index] = CellPin::Read(CellRead::Contribution(link));
    cell.pins = pins.into_boxed_slice();
    assert!(
        validate_cells(&graph, &omitted_guard, &monitor).is_err(),
        "weakening a source guard is rejected"
    );

    let mut wrong_polarity = cells.clone();
    let cell = wrong_polarity
        .iter_mut()
        .find(|cell| {
            cell.pins.iter().any(|pin| {
                matches!(
                    pin,
                    CellPin::Gate {
                        kind: CellGateKind::Invert,
                        ..
                    }
                )
            })
        })
        .unwrap();
    let mut pins = cell.pins.to_vec();
    let index = pins
        .iter()
        .position(|pin| {
            matches!(
                pin,
                CellPin::Gate {
                    kind: CellGateKind::Invert,
                    ..
                }
            )
        })
        .unwrap();
    let CellPin::Gate { inputs, .. } = pins[index].clone() else {
        unreachable!()
    };
    pins[index] = CellPin::Gate {
        kind: CellGateKind::Buffer,
        inputs,
    };
    cell.pins = pins.into_boxed_slice();
    assert!(
        validate_cells(&graph, &wrong_polarity, &monitor).is_err(),
        "inverting a presence predicate is rejected"
    );

    let mut missing_resource = cells.clone();
    let cell = missing_resource.iter_mut().find(|cell| !cell.resources.is_empty()).unwrap();
    cell.resources = Box::new([]);
    assert!(
        validate_cells(&graph, &missing_resource, &monitor).is_err(),
        "omitting a conserved resource output is rejected"
    );
}

#[test]
fn wrong_direction_and_missing_reset_coowners_do_not_form_cells() {
    let (graph, cells) = assert_all_fixture_actors_accounted("adder_1bit", 13);
    let monitor = TaskMonitor::default();
    let target = cells
        .iter()
        .find(|cell| cell.uses.len() > 1)
        .expect("fixture has a shared reset/resource ownership group");
    let actor = target.uses[0].actor;
    let observer = target.uses[0].reset;

    let mut wrong_direction = graph.clone();
    change_reset_direction(&mut wrong_direction, observer, BlockFace::Top);
    let wrong_direction_cells = discover_cells(&wrong_direction, &monitor).unwrap();
    assert!(
        wrong_direction_cells
            .cells
            .iter()
            .all(|cell| cell.uses.iter().all(|use_| use_.actor != actor)),
        "a reset observer facing away from the base cannot admit its actor"
    );

    let mut missing_reset = graph;
    erase_reset_recognition(&mut missing_reset, observer);
    let missing_reset_cells = discover_cells(&missing_reset, &monitor).unwrap();
    assert!(
        missing_reset_cells
            .declined
            .iter()
            .any(|decline| decline.actors.contains(&actor) && decline.reason == CellDeclineReason::UnrecognizedActor(actor)),
        "an ordinary co-owner without reset provenance declines the entire shared cell"
    );
    assert!(
        missing_reset_cells
            .cells
            .iter()
            .all(|cell| cell.uses.iter().all(|use_| use_.actor != actor)),
        "a missing-reset co-owner is never partially admitted"
    );
}

#[test]
fn conflicting_common_far_source_is_declined_as_one_ownership_group() {
    let (mut graph, cells) = assert_all_fixture_actors_accounted("adder_1bit", 13);
    let monitor = TaskMonitor::default();
    let mut conflict = None;
    for cell in cells.iter().filter(|cell| cell.uses.len() > 1 && !cell.resources.is_empty()) {
        for first in cell.uses.iter() {
            let PistonLane::SingleResource { far: first_far } = graph.pistons[first.actor.index()].lane else {
                continue;
            };
            for second in cell.uses.iter().filter(|second| second.actor != first.actor) {
                let PistonLane::SingleResource { far: second_far } = graph.pistons[second.actor.index()].lane else {
                    continue;
                };
                if first_far != second_far {
                    conflict = Some((cell.resources[0].resource, first.actor, second.actor, first_far, second_far));
                    break;
                }
            }
            if conflict.is_some() {
                break;
            }
        }
        if conflict.is_some() {
            break;
        }
    }
    let (resource, first, second, first_far, second_far) = conflict.expect("adder has shared owners with distinct source contacts");
    let contacts = &mut graph.resources[resource.index()].contacts;
    let mut widened = contacts.to_vec();
    for contact in [
        graph.pistons[first.index()].near,
        first_far,
        graph.pistons[second.index()].near,
        second_far,
    ] {
        if !widened.contains(&contact) {
            widened.push(contact);
        }
    }
    *contacts = widened.into_boxed_slice();

    let discovery = discover_cells(&graph, &monitor).unwrap();
    assert!(
        discovery.declined.iter().any(|decline| {
            decline.reason == CellDeclineReason::DifferentPullSources(resource) && decline.actors.contains(&first) && decline.actors.contains(&second)
        }),
        "one conserved payload cannot have two common Far sources in the same cell"
    );
    assert!(
        discovery
            .cells
            .iter()
            .all(|cell| cell.uses.iter().all(|use_| use_.actor != first && use_.actor != second))
    );
}

#[test]
fn malformed_cell_unary_arity_and_forward_pin_references_are_rejected() {
    let (graph, cells) = assert_all_fixture_actors_accounted("adder_1bit", 13);
    let cell = cells.first().unwrap();
    let (program, state) = graph.lower().unwrap();
    let backend = GraphBackend::new(program, state, 0).unwrap();
    let active = BTreeSet::new();
    let mut values = Vec::new();

    let mut wrong_arity = cell.clone();
    let mut pins = wrong_arity.pins.to_vec();
    let target = pins.iter().position(|pin| matches!(pin, CellPin::Gate { .. })).unwrap();
    pins[target] = CellPin::Gate {
        kind: CellGateKind::Buffer,
        inputs: Box::new([]),
    };
    wrong_arity.pins = pins.into_boxed_slice();
    assert_eq!(wrong_arity.evaluate(&mut values, |_| false), Err("unary cell gate has the wrong arity"));

    let mut forward_reference = cell.clone();
    let mut pins = forward_reference.pins.to_vec();
    let target = pins.iter().position(|pin| matches!(pin, CellPin::Gate { .. })).unwrap();
    pins[target] = CellPin::Gate {
        kind: CellGateKind::All,
        inputs: vec![CellPinId(u32::MAX)].into_boxed_slice(),
    };
    forward_reference.pins = pins.into_boxed_slice();
    assert_eq!(
        forward_reference.evaluate(&mut values, |_| false),
        Err("cell pin references a missing or later operand")
    );
    assert!(!evaluate(cell, &backend, &active).is_empty());
}

#[test]
fn independent_bud_memory_is_not_admitted_as_a_stationary_instant_cell() {
    use crate::redpiler::CompilerOptions;

    let circuit = boundary_contract_tests::load_mechanism("bud_instantmemorycellobserverupdate");
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
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
    let discovery = discover_cells(&prepared.graph, &monitor).unwrap();
    let admitted: BTreeSet<_> = discovery.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect();
    let memory_position = circuit.place(local(&circuit.ports["observations"]["memory"]));
    let memory_actor = prepared
        .graph
        .pistons
        .iter()
        .position(|actor| actor.position == memory_position)
        .map(|index| PistonId::new(index as u32))
        .expect("fixture memory site is a piston actor");
    assert!(
        !admitted.contains(&memory_actor),
        "ordinary BUD memory actor is outside instant ownership"
    );
}
