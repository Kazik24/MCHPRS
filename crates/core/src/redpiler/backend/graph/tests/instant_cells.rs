//! Equation checks for graph-derived stationary pull cells.
use super::*;
use crate::redpiler::analysis::instant::cells::{CellDeclineReason, discover_cells, validate_cells};
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
        let checks = check_admitted_inputs(&graph, &cells);
        assert!(checks > actors, "{id} exercises strength, guard and contact-state variants");
        println!("{id}: cells={}, actors={actors}, typed Graph comparisons={checks}", cells.len());
    }
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
