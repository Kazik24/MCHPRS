//! Real block layouts exercise entry alternatives, shared ownership and retained outside walks.
use super::*;
use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};

fn add_control(world: &mut PlotWorld, position: BlockPos) {
    world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        position,
        Block::Lever {
            lever: Lever {
                face: LeverFace::Floor,
                facing: BlockDirection::North,
                powered: true,
            },
        },
    );
}

fn compile_world(world: &PlotWorld) -> GraphBackend {
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let catalog = infer_boundaries(&prepared.graph, &monitor).unwrap();
    let plan = compile_layers(&prepared.graph, &catalog, &monitor).unwrap();
    assert!(!plan.regions.is_empty(), "expected a compiled region: {:?}", plan.declines);
    prepared.graph.plans.instant.boundaries = Some(catalog);
    prepared.graph.plans.instant.layers = Some(plan);
    prepared.graph.validate().unwrap();
    crate::redpiler::analysis::export_json::from_graph(&prepared.report, &prepared.graph).unwrap();
    let (program, state) = prepared.graph.lower().unwrap();
    assert!(!program.instant_kernels().is_empty(), "the artifact must actually execute");
    GraphBackend::new_unactivated(program, state, 0).unwrap()
}

fn compile_o6_world(world: &PlotWorld, incremental: bool, required_pull_actors: usize) -> GraphBackend {
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(world, BOUNDS, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(6),
        stable_instant_inputs: true,
        budget_multiplier: 8,
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
    assert!(
        prepared.graph.plans.instant.layers.is_some(),
        "O6 stable compilation should install a layer plan"
    );
    assert!(prepared.graph.plans.settled_circuit.is_some(), "O6 should install its logical marker");
    let layer_summary = prepared
        .graph
        .plans
        .instant
        .layers
        .as_ref()
        .map(|plan| format!("regions={} declines={:?}", plan.regions.len(), plan.declines));
    let (program, state) = prepared.graph.lower().unwrap();
    let kernel_summary: Vec<_> = program
        .instant_kernels()
        .iter()
        .map(|kernel| match &kernel.program {
            KernelProgram::PullWave { program, .. } => format!("PullWave({} expr)", program.expressions.len()),
            KernelProgram::Events(entries) => format!("Events({} entries)", entries.len()),
        })
        .collect();
    assert!(
        if required_pull_actors == 0 {
            true
        } else {
            program
                .instant_kernels()
                .iter()
                .any(|kernel| matches!(&kernel.program, KernelProgram::PullWave { .. }) && kernel.actors.len() >= required_pull_actors)
        },
        "O6 kernel profitability/admission mismatch; layer_plan={layer_summary:?}, kernels={kernel_summary:?}"
    );
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    if !incremental {
        // Keep the identical O6 artifact and ownership; disable only its test cache.
        backend.disable_incremental_values();
    }
    backend
}

fn assert_backend_state_equal(left: &GraphBackend, right: &GraphBackend, label: &str) {
    assert_eq!(left.state, right.state, "{label}: component/piston/resource states");
    assert_eq!(left.future.pending, right.future.pending, "{label}: pending memberships");
    assert_eq!(left.future.pending_nodes, right.future.pending_nodes, "{label}: pending node counts");
    assert_eq!(left.future.piston_requests, right.future.piston_requests, "{label}: piston FIFO");
    assert_eq!(
        left.future.deferred_pistons, right.future.deferred_pistons,
        "{label}: deferred piston work"
    );
    let scheduled = |backend: &GraphBackend| {
        backend
            .future
            .scheduler
            .iter()
            .map(|(tick, delay, priority)| (*tick, delay, priority))
            .collect::<Vec<_>>()
    };
    assert_eq!(scheduled(left), scheduled(right), "{label}: scheduler entries and priorities");
    assert_eq!(
        (
            left.future.clock.logical_tick,
            left.future.clock.phase,
            left.future.clock.scheduled_advanced
        ),
        (
            right.future.clock.logical_tick,
            right.future.clock.phase,
            right.future.clock.scheduled_advanced
        ),
        "{label}: graph tick phase"
    );
}

fn paired_ticks(
    logical: &mut GraphBackend,
    exact: &mut GraphBackend,
    native: &mut PlotWorld,
    logical_world: &mut PlotWorld,
    exact_world: &mut PlotWorld,
    positions: &[BlockPos],
    count: usize,
    label: &str,
) {
    for tick in 0..count {
        logical.tick().unwrap();
        exact.tick().unwrap();
        native.tick_interpreted();
        assert_backend_state_equal(logical, exact, &format!("{label} tick {tick}"));
        check_boundary(logical, native, logical_world, positions, &format!("{label} logical tick {tick}")).unwrap();
        check_boundary(exact, native, exact_world, positions, &format!("{label} exact tick {tick}")).unwrap();
    }
}

fn check_ticks(backend: &mut GraphBackend, native: &mut PlotWorld, projected: &mut PlotWorld, label: &str) {
    let positions: Vec<_> = backend.program.sites().iter().map(|site| site.position).collect();
    for tick in 0..24 {
        backend.tick().unwrap();
        native.tick_interpreted();
        check_boundary(backend, native, projected, &positions, &format!("{label} tick={tick}")).unwrap();
    }
}

fn turn_off(backend: &mut GraphBackend, native: &mut PlotWorld, position: BlockPos) {
    backend.use_block(position).unwrap();
    native_step(native, Step::SetLever { position, powered: false });
}

#[test]
fn captured_wire_request_executes_a_kernel_and_rechecks_restored_power() {
    let source = CONTROL.offset(BlockFace::West);
    for restore in [false, true] {
        let mut world = wire_controlled_cell_world();
        let mut native = wire_controlled_cell_world();
        let mut backend = compile_world(&world);
        assert!(backend.program.instant_kernels().iter().any(|kernel| {
            let KernelProgram::PullWave { program, .. } = &kernel.program else {
                return false;
            };
            program
                .inputs
                .iter()
                .any(|input| matches!(input.source, LayerInputSource::BoundaryRequest { .. }))
        }));
        turn_off(&mut backend, &mut native, source);
        assert!(backend.future.piston_requests.front().unwrap().source.is_some());
        if restore {
            backend.use_block(source).unwrap();
            native_step(
                &mut native,
                Step::SetLever {
                    position: source,
                    powered: true,
                },
            );
        }
        check_ticks(&mut backend, &mut native, &mut world, "captured request");
        assert!(backend.execution_counts().instant_kernel_invocations > 0);
    }
}

#[test]
fn o6_unproven_request_origin_uses_exact_acceptance() {
    let initial = chain_world(true);
    let mut logical = compile_o6_world(&initial, true, 2);
    let mut exact = compile_o6_world(&initial, false, 2);
    let mut native = chain_world(true);
    let mut logical_world = chain_world(true);
    let mut exact_world = chain_world(true);
    let positions: Vec<_> = logical.program.sites().iter().map(|site| site.position).collect();
    let source = CONTROL;
    turn_off(&mut logical, &mut native, source);
    exact.use_block(source).unwrap();
    // Imported/direct samples have no certified route provenance.
    logical.future.piston_requests.front_mut().unwrap().source = None;
    exact.future.piston_requests.front_mut().unwrap().source = None;
    paired_ticks(
        &mut logical,
        &mut exact,
        &mut native,
        &mut logical_world,
        &mut exact_world,
        &positions,
        24,
        "uncertified origin",
    );
    assert_eq!(logical.execution_counts().logical_kernel_evaluations, 0);
    assert!(logical.execution_counts().piston_actions > 0);
}

#[test]
fn o6_singleton_private_cell_keeps_exact_event_or_cpu_path() {
    let initial = direct_cell_world();
    let mut logical = compile_o6_world(&initial, true, 0);
    assert!(
        logical
            .program
            .instant_kernels()
            .iter()
            .all(|kernel| !matches!(&kernel.program, KernelProgram::PullWave { .. }))
    );
    assert!(
        logical
            .program
            .instant_kernels()
            .iter()
            .any(|kernel| matches!(&kernel.program, KernelProgram::Events(_)))
            || logical
                .program
                .pistons()
                .iter()
                .enumerate()
                .any(|(index, _)| { logical.program.cpu_kernel_entry(PistonId::new(index as u32)).is_some() }),
        "the singleton private cell remains on its exact event/CPU path"
    );
    let mut native = direct_cell_world();
    let mut projected = direct_cell_world();
    turn_off(&mut logical, &mut native, CONTROL);
    check_ticks(&mut logical, &mut native, &mut projected, "O6 singleton exact path");
    assert_eq!(logical.execution_counts().logical_kernel_evaluations, 0);
    assert!(logical.execution_counts().piston_actions > 0);
}

#[test]
fn o6_incremental_pull_wave_matches_exact_native_and_reset_continuation() {
    let source = CONTROL;
    let initial = chain_world(true);
    let mut logical = compile_o6_world(&initial, true, 2);
    let mut exact = compile_o6_world(&initial, false, 2);
    assert!(logical.program.settled_circuit().is_some());
    assert!(exact.program.settled_circuit().is_some());

    let positions: Vec<_> = logical.program.sites().iter().map(|site| site.position).collect();
    let mut native = chain_world(true);
    let mut logical_world = chain_world(true);
    let mut exact_world = chain_world(true);

    // Queue a direct boundary-triggered pull, then restore source power before entry.
    turn_off(&mut logical, &mut native, source);
    exact.use_block(source).unwrap();
    assert!(!logical.future.piston_requests.is_empty());
    logical.use_block(source).unwrap();
    exact.use_block(source).unwrap();
    native_step(
        &mut native,
        Step::SetLever {
            position: source,
            powered: true,
        },
    );
    assert_eq!(logical.future.piston_requests, exact.future.piston_requests);

    let first_logical = logical.future.piston_requests.pop_front().unwrap();
    let first_exact = exact.future.piston_requests.pop_front().unwrap();
    assert_eq!(first_logical, first_exact);
    let state_before_attempt = logical.state.clone();
    let future_before_attempt = (
        logical.future.pending.clone(),
        logical.future.pending_nodes.clone(),
        logical.future.piston_requests.clone(),
        logical.future.deferred_pistons.clone(),
        logical
            .future
            .scheduler
            .iter()
            .map(|(tick, delay, priority)| (*tick, delay, priority))
            .collect::<Vec<_>>(),
        (
            logical.future.clock.logical_tick,
            logical.future.clock.phase,
            logical.future.clock.scheduled_advanced,
        ),
    );
    let actions_before_attempt = logical.execution_counts().piston_actions;
    let operations_before = logical.execution_counts().logical_kernel_operations;
    let logical_handled = logical.run_instant_kernel(first_logical).unwrap();
    let exact_handled = exact.run_instant_kernel(first_exact).unwrap();
    assert_eq!(logical_handled, exact_handled, "logical and exact PullWave handling result");
    assert_eq!(
        logical.state, state_before_attempt,
        "restored source power causes no component/piston/resource changes"
    );
    assert_eq!(
        (
            logical.future.pending.clone(),
            logical.future.pending_nodes.clone(),
            logical.future.piston_requests.clone(),
            logical.future.deferred_pistons.clone(),
            logical
                .future
                .scheduler
                .iter()
                .map(|(tick, delay, priority)| (*tick, delay, priority))
                .collect::<Vec<_>>(),
            (
                logical.future.clock.logical_tick,
                logical.future.clock.phase,
                logical.future.clock.scheduled_advanced,
            ),
        ),
        future_before_attempt,
        "restored source power causes no scheduled work"
    );
    assert_eq!(
        logical.execution_counts().piston_actions,
        actions_before_attempt,
        "restored source power accepts no piston action"
    );
    assert_backend_state_equal(&logical, &exact, "first restored-power veto");
    let operations_after_first = logical.execution_counts().logical_kernel_operations;
    assert!(logical.execution_counts().logical_kernel_evaluations > 0);
    let logical_repeat_handled = logical.run_instant_kernel(first_logical).unwrap();
    let exact_repeat_handled = exact.run_instant_kernel(first_exact).unwrap();
    assert_eq!(logical_repeat_handled, exact_repeat_handled, "repeated logical and exact handling result");
    assert_eq!(logical.state, state_before_attempt, "repeated restored-power entry remains a no-op");
    assert_eq!(
        (
            logical.future.pending.clone(),
            logical.future.pending_nodes.clone(),
            logical.future.piston_requests.clone(),
            logical.future.deferred_pistons.clone(),
            logical
                .future
                .scheduler
                .iter()
                .map(|(tick, delay, priority)| (*tick, delay, priority))
                .collect::<Vec<_>>(),
            (
                logical.future.clock.logical_tick,
                logical.future.clock.phase,
                logical.future.clock.scheduled_advanced,
            ),
        ),
        future_before_attempt,
        "repeated restored-power entry schedules no work"
    );
    assert_eq!(logical.execution_counts().piston_actions, actions_before_attempt);
    assert_eq!(
        logical.execution_counts().logical_kernel_operations,
        operations_after_first,
        "an identical stationary entry changes no cached expression values"
    );
    assert!(operations_after_first >= operations_before);
    assert_backend_state_equal(&logical, &exact, "repeated restored-power veto");
    logical.future.piston_requests.push_front(first_logical);
    exact.future.piston_requests.push_front(first_exact);

    paired_ticks(
        &mut logical,
        &mut exact,
        &mut native,
        &mut logical_world,
        &mut exact_world,
        &positions,
        24,
        "restored-power veto",
    );

    // Run two changing-input pull cycles through the same O6 PullWave evaluator.
    for cycle in 0..2 {
        turn_off(&mut logical, &mut native, source);
        exact.use_block(source).unwrap();
        paired_ticks(
            &mut logical,
            &mut exact,
            &mut native,
            &mut logical_world,
            &mut exact_world,
            &positions,
            24,
            &format!("logical pull cycle {cycle}"),
        );

        logical.use_block(source).unwrap();
        exact.use_block(source).unwrap();
        native_step(
            &mut native,
            Step::SetLever {
                position: source,
                powered: true,
            },
        );
        paired_ticks(
            &mut logical,
            &mut exact,
            &mut native,
            &mut logical_world,
            &mut exact_world,
            &positions,
            24,
            &format!("logical restore cycle {cycle}"),
        );
    }
    assert!(logical.execution_counts().logical_kernel_evaluations > 1);
    assert!(logical.execution_counts().logical_kernel_operations > 0);
    assert!(logical.execution_counts().instant_kernel_invocations >= 2);
    assert_eq!(exact.execution_counts().logical_kernel_evaluations, 0);
    assert_backend_state_equal(&logical, &exact, "before interpreter handoff");

    logical.reset(&mut logical_world);
    exact.reset(&mut exact_world);
    for &position in &positions {
        assert_eq!(
            logical_world.get_block(position),
            exact_world.get_block(position),
            "handoff block at {position:?}"
        );
        assert_eq!(
            logical_world.get_block(position),
            native.get_block(position),
            "native handoff block at {position:?}"
        );
    }
    for tick in 0..12 {
        logical_world.tick_interpreted();
        exact_world.tick_interpreted();
        native.tick_interpreted();
        for &position in &positions {
            assert_eq!(
                logical_world.get_block(position),
                exact_world.get_block(position),
                "continuation tick {tick} at {position:?}"
            );
            assert_eq!(
                logical_world.get_block(position),
                native.get_block(position),
                "native continuation tick {tick} at {position:?}"
            );
        }
    }
}

#[test]
fn either_boundary_route_can_start_a_direct_instant_chain() {
    let alternate = BASE.offset(BlockFace::North);
    for order in [[CONTROL, alternate], [alternate, CONTROL]] {
        let mut world = direct_chain_world();
        add_control(&mut world, alternate);
        let mut backend = compile_world(&world);
        assert!(backend.program.instant_kernels()[0].entries.len() >= 2);
        let mut native = direct_chain_world();
        add_control(&mut native, alternate);
        turn_off(&mut backend, &mut native, order[0]);
        turn_off(&mut backend, &mut native, order[1]);
        check_ticks(&mut backend, &mut native, &mut world, "alternative entry");
        assert!(backend.execution_counts().instant_kernel_invocations > 0);
    }
}

#[test]
fn singleton_retains_its_power_output_and_outside_dust_walk() {
    let mut world = output_world();
    let mut backend = compile_world(&world);
    assert!(backend.program.instant_kernels()[0].wires.is_empty(), "the outside wire was not erased");
    let mut native = output_world();
    turn_off(&mut backend, &mut native, CONTROL);
    check_ticks(&mut backend, &mut native, &mut world, "ordinary output");
    assert!(backend.execution_counts().instant_kernel_invocations > 0);
    assert!(backend.execution_counts().wire_walks > 0, "outside propagation remains observable");
}

fn output_world() -> PlotWorld {
    let mut world = direct_cell_world();
    let output = BlockPos::new(11, 20, 8);
    world.set_block(output.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        output,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                15,
            ),
        },
    );
    world
}

#[test]
fn wired_observer_reset_remains_an_outside_clock_for_a_compiled_pull() {
    let build = || {
        let mut world = direct_cell_world();
        for position in [
            BASE.offset(BlockFace::East),
            BASE.offset(BlockFace::East).offset(BlockFace::East),
            BASE.offset(BlockFace::Top),
        ] {
            world.set_block(position, Block::Air);
        }
        world.set_block(
            BASE,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::Down,
                    sticky: true,
                    extended: true,
                },
            },
        );
        world.set_block(
            BASE.offset(BlockFace::Bottom),
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: BlockFacing::Down,
                    sticky: true,
                    short: false,
                },
            },
        );
        world.set_block(BASE.offset(BlockFace::Bottom).offset(BlockFace::Bottom), Block::RedstoneBlock);
        world.set_block(
            BASE.offset(BlockFace::South),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::North,
                    powered: false,
                },
            },
        );
        world.set_block(BASE + BlockPos::new(0, 0, 2), Block::Stone {});
        world.set_block(BASE + BlockPos::new(0, 1, 1), Block::Stone {});
        world.set_block(
            BASE + BlockPos::new(0, 1, 2),
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    0,
                ),
            },
        );
        world
    };
    let mut world = build();
    let mut native = build();
    let mut backend = compile_world(&world);
    assert!(backend.program.instant_kernels()[0].wires.is_empty());
    turn_off(&mut backend, &mut native, CONTROL);
    check_ticks(&mut backend, &mut native, &mut world, "wired reset");
    assert!(backend.execution_counts().instant_kernel_invocations > 0);
    assert!(backend.execution_counts().wire_walks > 0, "the reset clock remains ordinary Graph work");
}

#[test]
fn shared_payload_keeps_one_token_and_the_real_fifo_winner() {
    let control = BlockPos::new(10, 20, 5);
    for controls in [vec![CONTROL], vec![control], vec![CONTROL, control], vec![control, CONTROL]] {
        let mut world = shared_world();
        let mut backend = compile_world(&world);
        assert_eq!(backend.state.resources.len(), 1);
        assert_eq!(backend.program.instant_kernels()[0].actors.len(), 2);
        let mut native = shared_world();
        for &selected in &controls {
            turn_off(&mut backend, &mut native, selected);
        }
        check_ticks(&mut backend, &mut native, &mut world, "shared token");
        if controls.len() == 1 {
            assert!(backend.execution_counts().instant_kernel_invocations > 0);
        } else {
            assert_eq!(
                backend.execution_counts().instant_kernel_invocations,
                0,
                "mixed entries keep the actual FIFO on Graph"
            );
        }
    }
}

#[test]
fn singleton_kernels_leave_unrelated_queued_actions_in_order() {
    let build = || {
        let mut world = direct_cell_world();
        for position in [
            BASE,
            BASE.offset(BlockFace::East),
            BASE.offset(BlockFace::East).offset(BlockFace::East),
            BASE.offset(BlockFace::Top),
            BASE.offset(BlockFace::Top).offset(BlockFace::Top),
            CONTROL,
            CONTROL.offset(BlockFace::Bottom),
        ] {
            let block = world.get_block(position);
            world.set_block(position + BlockPos::new(0, 0, 5), block);
        }
        world
    };
    for controls in [[CONTROL, CONTROL + BlockPos::new(0, 0, 5)], [CONTROL + BlockPos::new(0, 0, 5), CONTROL]] {
        let mut world = build();
        let mut native = build();
        let mut backend = compile_world(&world);
        assert_eq!(backend.program.instant_kernels().len(), 2);
        for control in controls {
            turn_off(&mut backend, &mut native, control);
        }
        assert_eq!(backend.future.piston_requests.len(), 2);
        check_ticks(&mut backend, &mut native, &mut world, "unrelated queued actions");
        assert_eq!(backend.execution_counts().instant_kernel_invocations, 2);
    }
}

fn shared_world() -> PlotWorld {
    let second = BlockPos::new(10, 20, 6);
    let control = BlockPos::new(10, 20, 5);
    let mut world = direct_cell_world();
    world.set_block(
        second,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::South,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        second.offset(BlockFace::South),
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::South,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(
        second.offset(BlockFace::Top),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(second.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {});
    add_control(&mut world, control);
    world
}
