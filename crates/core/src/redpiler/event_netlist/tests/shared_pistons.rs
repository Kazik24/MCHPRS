#[test]
fn opposing_physical_pistons_compile_to_one_resource_identity() {
    use crate::plot::PlotWorld;
    use crate::redpiler::backend::graph::wire_probe::physical_program;
    use crate::world::World;
    use crate::world::storage::Chunk;
    use mchprs_blocks::{
        BlockFacing,
        blocks::{Block, RedstonePiston},
    };
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let bases = [mchprs_blocks::BlockPos::new(5, 20, 5), mchprs_blocks::BlockPos::new(5, 20, 9)];
    let facings = [BlockFacing::South, BlockFacing::North];
    for (base, facing) in bases.into_iter().zip(facings) {
        let piston = RedstonePiston {
            facing,
            sticky: true,
            extended: true,
        };
        world.set_block(base, Block::Piston { piston });
        world.set_block(base.offset(facing.into()), Block::PistonHead { head: piston.into() });
    }
    world.set_block(mchprs_blocks::BlockPos::new(5, 20, 7), Block::RedstoneBlock);
    let bounds = (mchprs_blocks::BlockPos::new(0, 0, 0), mchprs_blocks::BlockPos::new(15, 31, 15));
    let (program, state, _) = physical_program(&world, bounds).unwrap();
    assert_eq!(program.pistons().len(), 2);
    let runtime = CompiledPistonActors::compile(
        &program,
        &state,
        &crate::redpiler::physical_signal_network::EntryWork::capture(&world, bounds),
    )
    .unwrap();
    let first = runtime.resource_for_actor(super::super::program_graph::PistonId::new(0)).unwrap();
    let second = runtime.resource_for_actor(super::super::program_graph::PistonId::new(1)).unwrap();
    assert_eq!(first, second);
    assert!(runtime.resource_state(first).enabled);
    for order in [[0usize, 1usize], [1, 0]] {
        let mut executor = CompiledPistonActors::compile(
            &program,
            &state,
            &crate::redpiler::physical_signal_network::EntryWork::capture(&world, bounds),
        )
        .unwrap();
        executor.begin_computation_cycle();
        let requests = order.map(|index| {
            let id = super::super::program_graph::PistonId::new(index as u32);
            let actor = executor.actors[index].as_ref().unwrap();
            super::super::physical_signal_network::SignalEvent {
                piston: id,
                occurrence: index as u64,
                route: actor.routes[0],
                action: mchprs_world::PistonAction::Retract,
                piston_input: 0,
                captures: vec![],
                piston_guards: vec![(super::super::program_graph::GuardId::new(0), true)],
            }
        });
        assert_eq!(executor.drain(requests.into()).unwrap(), 1);
        let winner = order[0];
        assert_eq!(executor.successful_moves[winner], 1);
        assert_eq!(executor.successful_moves[order[1]], 0);
        assert!(executor.consumed[first.index()]);
        let final_state = executor.resource_state(first);
        assert!(!final_state.enabled);
        assert_eq!(
            final_state.at,
            if winner == 0 {
                executor.actors[0].as_ref().unwrap().near
            } else {
                executor.actors[1].as_ref().unwrap().near
            }
        );
        let mut stale = executor.clone();
        let mut piston = stale.signals.state().pistons[winner];
        piston.revision = piston.revision.wrapping_add(1);
        stale.signals.set_piston_state(winner, piston).unwrap();
        stale.advance_tick().unwrap();
        stale.advance_tick().unwrap();
        assert!(!stale.resource_state(first).enabled, "stale completion leaves resource untouched");
        executor.advance_tick().unwrap();
        executor.advance_tick().unwrap();
        assert!(executor.resource_state(first).enabled, "arrival restores the moved resource");
        assert!(executor.consumed[first.index()], "arrival does not rearm the cycle");
    }
}

#[test]
fn shared_route_keeps_piston_recipient_and_occurrence_identity() {
    use super::super::physical_signal_network::SignalEvent;
    use super::super::program_graph::{GuardId, PistonId};

    let events = [
        SignalEvent {
            piston: PistonId::new(0),
            occurrence: 41,
            route: 7,
            action: mchprs_world::PistonAction::Extend,
            piston_input: 15,
            captures: vec![],
            piston_guards: vec![(GuardId::new(0), true)],
        },
        SignalEvent {
            piston: PistonId::new(1),
            occurrence: 42,
            route: 7,
            action: mchprs_world::PistonAction::Extend,
            piston_input: 15,
            captures: vec![],
            piston_guards: vec![(GuardId::new(0), true)],
        },
    ];
    assert!(belongs_to_piston(&events[0], PistonId::new(0), &[7]));
    assert!(!belongs_to_piston(&events[1], PistonId::new(0), &[7]));
    assert!(belongs_to_piston(&events[1], PistonId::new(1), &[7]));
    assert_ne!(events[0].occurrence, events[1].occurrence);
}

#[test]
fn failed_departure_restores_signal_state_and_pending_requests() {
    use crate::redpiler::backend::graph::{
        piston_probe::tests::{BOUNDS, LEVER, fixture},
        wire_probe::physical_program,
    };
    use crate::redpiler::physical_signal_network::EntryWork;
    use crate::redpiler::program_graph::RouteId;
    let world = fixture();
    let (program, state, _) = physical_program(&world, BOUNDS).unwrap();
    let mut runtime = CompiledPistonActors::compile(&program, &state, &EntryWork::capture(&world, BOUNDS)).unwrap();
    std::sync::Arc::make_mut(&mut runtime.actors)[0].as_mut().unwrap().movement.near_changed = RouteId::new(u32::MAX);
    runtime.set_lever(LEVER, true).unwrap();
    let state_before = runtime.signals.state().clone();
    let pending_before: Vec<_> = runtime
        .pending
        .iter()
        .map(|event| (event.piston, event.route, event.occurrence, event.action))
        .collect();
    assert!(!pending_before.is_empty(), "lever produced a piston callback");
    let tick_before = runtime.signals.current_tick();
    assert_eq!(runtime.advance_tick().unwrap_err(), "signal callback route is unavailable");
    assert_eq!(runtime.signals.state(), &state_before);
    assert_eq!(runtime.signals.current_tick(), tick_before);
    assert_eq!(
        runtime
            .pending
            .iter()
            .map(|event| (event.piston, event.route, event.occurrence, event.action))
            .collect::<Vec<_>>(),
        pending_before
    );
    assert!(runtime.deferred.is_empty());
    assert!(runtime.consumed.iter().all(|used| !used));
    assert_eq!(runtime.successful_actions.iter().sum::<u64>(), 0);
}
