#[test]
fn test_pistion_source_topology_is_available_for_cycle_compilation() {
    use crate::redpiler::backend::graph::{piston_probe::tests::test_pistion_fixture, wire_probe::physical_program};
    use crate::redpiler::program_graph::{CallbackReceiver, PowerTarget};
    let (world, bounds, _) = test_pistion_fixture();
    let (program, state, _) = physical_program(&world, bounds).unwrap();
    assert_eq!(program.pistons().len(), 1);
    let piston = &program.pistons()[0];
    let resource = state
        .resources
        .iter()
        .enumerate()
        .find(|(_, state)| state.enabled && (state.at == piston.near || state.at == piston.far))
        .expect("clock lane contains a resource");
    let routes: Vec<_> = program
        .routes()
        .iter()
        .enumerate()
        .filter(|(_, route)| {
            route
                .deliveries
                .iter()
                .any(|delivery| matches!(delivery.receiver, Some(CallbackReceiver::PistonBase(_) | CallbackReceiver::PistonHead(_))))
        })
        .map(|(index, _)| index)
        .collect();
    let power: Vec<_> = program
        .power_links()
        .iter()
        .filter(|link| {
            matches!(link.to,
            PowerTarget::Input { node, .. } if node == piston.node)
        })
        .collect();
    assert!(!routes.is_empty(), "clock has source-qualified callback routes");
    assert!(!power.is_empty(), "clock has represented power dependencies");
    println!(
        "clock {:?}, lane {:?}, resource {:?}, callback routes {:?}, power links {:?}",
        piston.position, piston.lane, resource, routes, power
    );
    for link in &power {
        println!(
            "power site {:?}, guard {:?}",
            program.sites()[link.from.index()],
            program.guards()[link.guard.index()]
        );
        for child in program
            .power_links()
            .iter()
            .filter(|child| matches!(child.to, PowerTarget::Conductor { site, .. } if site == link.from))
        {
            println!(
                "conductor input {:?}, site {:?}, guard {:?}",
                child,
                program.sites()[child.from.index()],
                program.guards()[child.guard.index()]
            );
        }
    }
    let compiled = PhysicalPiston::compile(&program, &state, piston.position).unwrap();
    let (world, bounds, sources) = test_pistion_fixture();
    use crate::redpiler::components::NodeType;
    use crate::redpiler::program_graph::CircuitNodeKind;
    let output = program
        .nodes()
        .iter()
        .find_map(|node| match node.kind {
            CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => node.position,
            _ => None,
        })
        .expect("fixture has a repeater");
    let repeater = super::super::event_repeater::PhysicalRepeater::compile(&program, &state, output, 0).unwrap();
    let mut scope = PhysicalScope {
        piston: compiled,
        output: repeater,
    };
    let trial =
        &crate::redpiler::backend::graph::piston_probe::probe_clock_outputs(&world, bounds, &sources, piston.position, &[output], 12).unwrap()[0];
    for attempt in trial.attempts.iter().filter(|attempt| attempt.accepted) {
        let observed = &attempt.outputs[0];
        assert!(
            scope
                .notify(attempt.route, u8::from(attempt.powered) * 15, observed.input_strength, observed.locked)
                .unwrap()
        );
        assert_eq!(scope.piston.position(), attempt.after.as_ref().unwrap().position);
    }
    let _ = scope.tick();
}

#[test]
fn autonomous_piston_owns_its_repeater_scheduler_and_keeps_clock_running() {
    use crate::redpiler::backend::graph::{
        piston_probe::{probe_clock_outputs, tests::test_pistion_fixture},
        wire_probe::physical_program,
    };
    use crate::redpiler::components::NodeType;
    use crate::redpiler::program_graph::{CircuitNodeKind, ComponentState};

    let (world, bounds, sources) = test_pistion_fixture();
    let (program, state, _) = physical_program(&world, bounds).unwrap();
    let clock = program.pistons()[0].position;
    let outputs = program
        .nodes()
        .iter()
        .filter_map(|node| match node.kind {
            CircuitNodeKind::Electrical(NodeType::Repeater { .. }) => node.position,
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!outputs.is_empty());
    let expected = probe_clock_outputs(&world, bounds, &sources, clock, &outputs, 12).unwrap().remove(0);
    let falling = expected.clock_retractions.len();
    assert!(falling >= 2, "fixture oracle has repeated falling edges");

    let source_node = program.position_index()[&sources[0]].index();
    let source_powered = matches!(&state.components[source_node], ComponentState::Electrical(source) if source.powered);
    assert!(!source_powered, "fixture input starts off");
    let mut runtime = AutonomousPiston::compile(
        &program,
        &state,
        &crate::redpiler::physical_signal_network::EntryWork::capture(&world, bounds),
        clock,
    )
    .unwrap();
    runtime.set_lever(sources[0], !source_powered).unwrap();
    let final_edge = expected
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.actor == clock && attempt.accepted)
        .unwrap();
    for tick in 1..=12 {
        runtime.advance_tick().unwrap();
        if tick == final_edge.tick {
            for observation in &final_edge.outputs {
                let node = program.position_index()[&observation.position].index();
                let ComponentState::Electrical(output) = &runtime.state().components[node] else {
                    panic!("readout state is unavailable")
                };
                assert_eq!(output.powered, observation.powered, "clock readout at {}", observation.position);
            }
        }
    }

    let native_moves = expected
        .attempts
        .iter()
        .filter(|attempt| attempt.actor == clock && attempt.accepted)
        .count();
    assert_eq!(
        runtime.successful_moves() as usize,
        native_moves,
        "the actor processes each accepted move once"
    );
    assert!(
        runtime.successful_moves() >= (falling * 2) as u64,
        "the active source keeps the clock cycling"
    );
    let resource = runtime.state().resources[runtime.resource];
    let native = &expected.final_resources[runtime.resource];
    assert_eq!(program.contacts()[resource.at.index()].position, native.position);
    assert_eq!(resource.enabled, native.enabled);
    assert!(matches!(&runtime.state().components[source_node], ComponentState::Electrical(source) if source.powered));
}

#[test]
fn observed_physical_piston_request_moves_resource_in_event_runtime() {
    use crate::redpiler::backend::graph::piston_probe::{
        probe,
        tests::{ACTOR, BOUNDS, FAR, LEVER, NEAR, fixture},
    };
    let world = fixture();
    let trials = probe(&world, BOUNDS, &[LEVER], 4).unwrap();
    let trial = &trials[0];
    let mut runtime = EventNetlist::from_piston_trial(trial).unwrap();
    assert_eq!(trial.attempts[0].actor, ACTOR);
    assert_eq!(trial.initial_resources[0].position, NEAR);
    assert_eq!(trial.final_resources[0].position, FAR);
    let power = u8::from(trial.attempts[0].powered) * 15;
    runtime.frame(&[power], &[], &[]).unwrap();
    assert_eq!(runtime.contacts, [0], "no represented request means no movement");
    runtime.frame(&[power], &[0], &[]).unwrap();
    assert_eq!(
        runtime.contacts,
        [1],
        "observed request moves the same resource to the observed destination"
    );
}
#[test]
fn test_pistion_clock_window_is_not_a_settled_single_transfer() {
    use crate::redpiler::backend::graph::piston_probe::{probe_clock_window, tests::test_pistion_fixture};
    use crate::world::World;
    use mchprs_blocks::{BlockPos, blocks::Block};
    let (world, bounds, sources) = test_pistion_fixture();
    let mut pistons = Vec::new();
    for y in bounds.0.y..=bounds.1.y {
        for z in bounds.0.z..=bounds.1.z {
            for x in bounds.0.x..=bounds.1.x {
                let pos = BlockPos::new(x, y, z);
                if matches!(world.get_block(pos), Block::Piston { .. }) {
                    pistons.push(pos);
                }
            }
        }
    }
    assert_eq!(pistons.len(), 1);
    let trials = probe_clock_window(&world, bounds, &sources, pistons[0], 12).unwrap();
    let trial = &trials[0];
    assert!(!trial.settled);
    assert!(trial.clock_retractions.len() >= 2);
    assert!(
        EventNetlist::from_piston_trial(trial).is_err(),
        "an active clock is not a settled single-transfer replay"
    );
    let first = &trial.attempts[trial.clock_retractions[0]];
    let mut edge = EventNetlist::from_clock_edge(trial).expect("first observed clock edge is representable");
    assert_eq!(edge.window_end_pending, trial.pending, "window-end pending work remains visible");
    assert_eq!((edge.source, edge.route, edge.actor), (first.source, first.route, first.actor));
    assert_eq!(edge.from, first.before.as_ref().unwrap().position);
    assert_eq!(edge.to, first.after.as_ref().unwrap().position);
    assert!(
        edge.fire(edge.source, edge.route + 1, 0).is_err(),
        "an unrepresented source route cannot trigger the move"
    );
    assert!(
        !edge.fire(edge.source, edge.route, 15).unwrap(),
        "live powered state rejects a stale retraction"
    );
    assert!(
        edge.fire(edge.source, edge.route, 0).unwrap(),
        "observed unpowered request moves at the falling edge"
    );
}
