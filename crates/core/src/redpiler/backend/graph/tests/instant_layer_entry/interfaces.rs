//! Real block layouts exercise entry alternatives, shared ownership and retained outside walks.
use super::super::super::logical_device::{ActiveFramePowerInput, FrameResetSource};
use super::*;
use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};

#[path = "region_boundary.rs"]
mod region_boundary;

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

fn compile_o6_without_pull_waves(world: &PlotWorld) -> GraphBackend {
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
    // Keep the exact same graph rewrites and native owners; omit only the layer artifact.
    prepared.graph.plans.instant.layers = None;
    let (program, state) = prepared.graph.lower().unwrap();
    GraphBackend::new_unactivated(program, state, 0).unwrap()
}

fn compile_graph_backend(world: &PlotWorld) -> GraphBackend {
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let (program, state) = prepared.graph.lower().unwrap();
    GraphBackend::new_unactivated(program, state, 0).unwrap()
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
        let counts = backend.execution_counts();
        if restore {
            assert_eq!(counts.instant_kernel_invocations, 0, "powered revalidation keeps the queued request native");
        } else {
            assert!(counts.instant_kernel_invocations > 0, "the unpowered singleton request executes its compiled route");
        }
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
    logical.future.piston_requests.set_front_source(None);
    exact.future.piston_requests.set_front_source(None);
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
    assert!(logical.future.piston_requests.push_front(first_logical));
    assert!(exact.future.piston_requests.push_front(first_exact));

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
fn input_restoration_reset_uses_live_power_and_preserves_native_fifo() {
    let second = BlockPos::new(11, 20, 8);
    let make_world = || {
        let mut world = direct_chain_world();
        world.set_block(second.offset(BlockFace::Top), Block::Air);
        world
    };

    let world = make_world();
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let cell_catalog = crate::redpiler::analysis::instant::cells::discover_cells(&prepared.graph, &monitor).unwrap();
    let logical_actor = PistonId::new(
        prepared
            .graph
            .pistons
            .iter()
            .position(|piston| piston.position == second)
            .expect("the downstream piston is present") as u32,
    );
    let logical_use = cell_catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter())
        .find(|use_| use_.actor == logical_actor)
        .expect("the downstream cell has a certified reset contract");
    let CellReset::InputRestoration {
        resource: logical_resource,
        contact: logical_contact,
        ..
    } = logical_use.reset
    else {
        panic!("the downstream actor must use the input-restoration reset fixture");
    };
    let (logical_program, logical_state) = prepared.graph.lower().unwrap();
    let mut logical = GraphBackend::new_unactivated(logical_program, logical_state, 0).unwrap();

    // The comparison backend remains ordinary Graph. Its input with the exact
    // reset contribution removed serves as the data-only boundary value.
    let mut exact = compile_graph_backend(&world);
    let exact_actor = PistonId::new(
        exact
            .program
            .pistons()
            .iter()
            .position(|piston| piston.position == second)
            .expect("the downstream piston is present in the oracle") as u32,
    );
    let exact_node = exact.program.pistons()[exact_actor.index()].node;
    let exact_prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let exact_cells = crate::redpiler::analysis::instant::cells::discover_cells(&exact_prepared.graph, &monitor).unwrap();
    let exact_use = exact_cells
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter())
        .find(|use_| use_.actor.index() < exact.program.pistons().len() && exact.program.pistons()[use_.actor.index()].position == second)
        .expect("the oracle has the same source-certified reset contract");
    let CellReset::InputRestoration {
        resource: exact_resource,
        contact: exact_contact,
        ..
    } = exact_use.reset
    else {
        panic!("the oracle actor must use input restoration");
    };
    let logical_node = logical.program.pistons()[logical_actor.index()].node;
    logical.active_frame_power_inputs.push(ActiveFramePowerInput {
        node: logical_node,
        port: InputPort::Main,
        data_strength: 0,
        reset: Some(FrameResetSource::InputRestoration {
            resource: logical_resource,
            contact: logical_contact,
        }),
    });
    let input_slot = logical.active_frame_power_inputs.len() - 1;
    let mut native = make_world();
    let mut logical_world = make_world();
    let mut exact_world = make_world();
    let positions: Vec<_> = logical.program.sites().iter().map(|site| site.position).collect();

    for cycle in 0..2 {
        for (powered, phase) in [(false, "retract"), (true, "restore/refire")] {
            if !powered {
                turn_off(&mut logical, &mut native, CONTROL);
                exact.use_block(CONTROL).unwrap();
            } else {
                logical.use_block(CONTROL).unwrap();
                exact.use_block(CONTROL).unwrap();
                native_step(
                    &mut native,
                    Step::SetLever {
                        position: CONTROL,
                        powered: true,
                    },
                );
            }
            assert_eq!(logical.future.piston_requests, exact.future.piston_requests);
            for tick in 0..24 {
                let data_strength = exact.input_strength_without_frame_reset(
                    exact_node,
                    InputPort::Main,
                    FrameResetSource::InputRestoration {
                        resource: exact_resource,
                        contact: exact_contact,
                    },
                );
                assert_eq!(data_strength, 0, "the direct restoration fixture has no independent data input");
                logical.active_frame_power_inputs[input_slot].data_strength = data_strength;
                assert_eq!(
                    logical.input_strength(logical_node, InputPort::Main),
                    exact.input_strength(exact_node, InputPort::Main),
                    "live reset contribution and data-only input at cycle {cycle} {phase} tick {tick}"
                );
                logical.tick().unwrap();
                exact.tick().unwrap();
                native.tick_interpreted();
                assert_backend_state_equal(&logical, &exact, &format!("cycle {cycle} {phase} tick {tick}"));
            }
        }
    }

    logical.flush(&mut logical_world, false);
    exact.flush(&mut exact_world, false);
    for &position in &positions {
        assert_eq!(
            logical_world.get_block(position),
            exact_world.get_block(position),
            "Graph block at {position:?}"
        );
        assert_eq!(
            logical_world.get_block(position),
            native.get_block(position),
            "native block at {position:?}"
        );
    }
    assert!(logical.execution_counts().piston_actions > 0);
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
        assert!(
            backend.execution_counts().instant_kernel_invocations > 0,
            "a matching source alternative may accelerate one owner while Graph preserves FIFO arbitration"
        );
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

fn shared_payload_single_wire_world() -> PlotWorld {
    let mut world = shared_world();
    // The shared payload block moves during the mechanism, so dust cannot sit on top of it.
    // Place one supported dust node beside the Far source instead.
    let payload = BlockPos::new(10, 20, 8);
    let wire = payload.offset(BlockFace::South);
    world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        wire,
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

fn shared_payload_with_foreign_fifo_cell_world() -> PlotWorld {
    let mut world = shared_payload_single_wire_multiconsumer_world();
    let base = BlockPos::new(13, 20, 13);
    let control = base.offset(BlockFace::West);
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        base.offset(BlockFace::East),
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::East,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(base.offset(BlockFace::East).offset(BlockFace::East), Block::RedstoneBlock);
    world.set_block(
        base.offset(BlockFace::Top),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(base.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {});
    add_control(&mut world, control);
    world
}

fn shared_payload_with_input_restoration_world() -> PlotWorld {
    let mut world = super::chain_world(false);
    let third = BlockPos::new(13, 20, 10);
    world.set_block(BlockPos::new(11, 21, 8), Block::Air);
    world.set_block(
        third,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::West,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        third.offset(BlockFace::West),
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::West,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(third.offset(BlockFace::West).offset(BlockFace::West), Block::RedstoneBlock);
    world.set_block(
        third.offset(BlockFace::Top),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(third.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {});
    world
}

#[test]
fn shared_input_restoration_cell_keeps_native_reset_and_fifo() {
    let mut world = shared_payload_with_input_restoration_world();
    let mut native = shared_payload_with_input_restoration_world();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
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
    prepared.graph.validate().expect("the O6 shared reset graph validates");
    let catalog = prepared.graph.plans.instant.boundaries.as_ref().expect("O6 boundary catalog");
    let plan = prepared.graph.plans.instant.layers.as_ref().expect("O6 layer plan");
    let second = BlockPos::new(11, 20, 8);
    let downstream = PistonId::new(
        prepared
            .graph
            .pistons
            .iter()
            .position(|piston| piston.position == second)
            .expect("the downstream shared owner is present") as u32,
    );
    let shared_cell = catalog
        .cells
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.uses.iter().any(|use_| use_.actor == downstream))
        .expect("the downstream owner is recognized");
    let (cell_id, cell) = shared_cell;
    assert!(cell.uses.len() >= 2, "the resource must remain a shared-owner contract");
    let CellReset::InputRestoration { notification, sample, .. } = &cell.uses.iter().find(|use_| use_.actor == downstream).unwrap().reset else {
        panic!("the downstream shared owner must use its certified input-restoration reset");
    };
    let region = plan
        .regions
        .iter()
        .find(|region| region.cells.contains(&(cell_id as u32)))
        .unwrap_or_else(|| panic!("the shared reset cell must compile as a finite region: {:?}", plan.declines));
    for occurrence in [*notification, *sample] {
        assert!(
            region.cold_lifecycle_occurrences.iter().any(|row| row.occurrence == occurrence),
            "the exact Graph-owned reset occurrence {occurrence:?} must remain in the region perimeter"
        );
    }

    let (program, state) = prepared.graph.lower().unwrap();
    let kernel = program
        .instant_kernels()
        .iter()
        .find(|kernel| matches!(&kernel.program, KernelProgram::PullWave { .. }) && kernel.actors.len() >= 2)
        .expect("the ordinary O6 pipeline must install an executable shared-owner PullWave");
    assert!(kernel.actors.contains(&downstream));
    let pull_actors = kernel.actors.to_vec();
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    backend.test_recorder.instant_kernel_activity = vec![[0; 7]; backend.program.pistons().len()];
    let positions: Vec<_> = backend.program.sites().iter().map(|site| site.position).collect();
    for cycle in 0..2 {
        turn_off(&mut backend, &mut native, CONTROL);
        for tick in 0..24 {
            backend.tick().unwrap();
            native.tick_interpreted();
            check_boundary(
                &backend,
                &native,
                &mut world,
                &positions,
                &format!("InputRestoration cycle={cycle} retract tick={tick}"),
            )
            .unwrap();
        }
        backend.use_block(CONTROL).unwrap();
        native_step(
            &mut native,
            Step::SetLever {
                position: CONTROL,
                powered: true,
            },
        );
        for tick in 0..24 {
            backend.tick().unwrap();
            native.tick_interpreted();
            check_boundary(
                &backend,
                &native,
                &mut world,
                &positions,
                &format!("InputRestoration cycle={cycle} restore tick={tick}"),
            )
            .unwrap();
        }
    }
    assert!(
        pull_actors
            .iter()
            .any(|actor| backend.test_recorder.instant_kernel_activity[actor.index()][6] > 0),
        "a bound PullWave actor must actually execute"
    );
}

fn shared_payload_single_wire_multiconsumer_world() -> PlotWorld {
    let mut world = shared_payload_single_wire_world();
    // A third, independent sticky cell reads the same dust node but owns a
    // different payload. Its Main/Quasi pins must be sampled at the actual FIFO
    // prefix after the shared-token producers have been considered.
    let base = BlockPos::new(9, 20, 9);
    let head = base.offset(BlockFace::West);
    let far = head.offset(BlockFace::West);
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::West,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        head,
        Block::PistonHead {
            head: RedstonePistonHead {
                facing: BlockFacing::West,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(far, Block::RedstoneBlock);
    world.set_block(
        base.offset(BlockFace::Top),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(base.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {});
    world
}

#[test]
fn shared_payload_single_wire_arrival_preserves_native_fifo_for_each_owner() {
    let controls = [CONTROL, BlockPos::new(10, 20, 5)];
    for control in controls {
        let mut world = shared_payload_single_wire_world();
        let mut native = shared_payload_single_wire_world();
        let mut backend = compile_o6_world(&world, true, 2);
        let wave = backend
            .program
            .instant_kernels()
            .iter()
            .find(|kernel| matches!(&kernel.program, KernelProgram::PullWave { .. }))
            .expect("the isolated shared-wire arrival must lower to a pull wave");
        assert_eq!(wave.actors.len(), 2, "the shared source keeps both real FIFO owners");
        assert_eq!(wave.wires.len(), 1, "only the certified source wire is replaced");

        turn_off(&mut backend, &mut native, control);
        check_ticks(&mut backend, &mut native, &mut world, "shared source arrival");
        assert!(backend.execution_counts().instant_kernel_invocations > 0);
        assert_eq!(backend.state.resources.len(), 1);
    }
}

#[test]
fn shared_wire_hot_entry_level_does_not_depend_on_saved_initial_power() {
    let mut world = shared_payload_single_wire_world();
    let wire = BlockPos::new(10, 20, 9);
    world.set_block(
        wire,
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

    let mut backend = compile_o6_world(&world, true, 2);
    let kernel = backend
        .program
        .instant_kernels()
        .iter()
        .find(|kernel| matches!(&kernel.program, KernelProgram::PullWave { .. }))
        .expect("the source and callback proof must admit this cold saved snapshot");
    assert_eq!(kernel.wires.len(), 1);
    assert_eq!(kernel.wires[0].1, 15, "the hot-entry level comes from the adjacent source proof");
    let wire_node = kernel.wires[0].0;

    backend.use_block(CONTROL).unwrap();
    let request = backend
        .future
        .piston_requests
        .pop_front()
        .expect("the control must queue an eligible retract");
    assert_eq!(request.action, mchprs_world::PistonAction::Retract);
    assert!(matches!(
        backend.state.components[wire_node.index()],
        ComponentState::Wire(wire) if wire.power == 0
    ));

    let state_before = backend.state.clone();
    let pending_before = backend.future.pending.clone();
    let pending_nodes_before = backend.future.pending_nodes.clone();
    let requests_before = backend.future.piston_requests.clone();
    let deferred_before = backend.future.deferred_pistons.clone();
    let scheduler_before = backend
        .future
        .scheduler
        .iter()
        .map(|(tick, delay, priority)| (*tick, delay, priority))
        .collect::<Vec<_>>();
    assert!(
        !backend.run_instant_kernel(request).unwrap(),
        "a stale saved wire level must fail the hot-entry guard"
    );
    assert_eq!(backend.state, state_before);
    assert_eq!(backend.future.pending, pending_before);
    assert_eq!(backend.future.pending_nodes, pending_nodes_before);
    assert_eq!(backend.future.piston_requests, requests_before);
    assert_eq!(backend.future.deferred_pistons, deferred_before);
    assert_eq!(
        backend
            .future
            .scheduler
            .iter()
            .map(|(tick, delay, priority)| (*tick, delay, priority))
            .collect::<Vec<_>>(),
        scheduler_before
    );
}

#[test]
fn shared_payload_single_wire_arrival_preserves_multi_cell_fifo_prefixes() {
    let mut world = shared_payload_single_wire_multiconsumer_world();
    let mut native = shared_payload_single_wire_multiconsumer_world();
    let mut backend = compile_o6_world(&world, true, 3);
    let wave = backend
        .program
        .instant_kernels()
        .iter()
        .find(|kernel| matches!(&kernel.program, KernelProgram::PullWave { .. }))
        .expect("the single-wire source and its independent consumer must compose");
    assert_eq!(wave.actors.len(), 3, "both shared owners and the private receiver stay in the FIFO wave");
    assert_eq!(wave.wires.len(), 1, "only the certified one-wire corridor is replaced");

    turn_off(&mut backend, &mut native, CONTROL);
    check_ticks(&mut backend, &mut native, &mut world, "shared single-wire multi-cell arrival");
    assert!(backend.execution_counts().instant_kernel_invocations > 0);
    assert_eq!(backend.state.resources.len(), 2, "the independent cell keeps its own payload");
}

#[test]
fn pull_wave_resumes_only_at_its_actual_fifo_requests() {
    let foreign_control = BlockPos::new(12, 20, 13);
    for interleave_foreign in [false, true] {
        let mut projected = shared_payload_with_foreign_fifo_cell_world();
        let mut exact_native = shared_payload_with_foreign_fifo_cell_world();
        let mut exact_projection = shared_payload_with_foreign_fifo_cell_world();
        let mut native = shared_payload_with_foreign_fifo_cell_world();
        let mut compiled = compile_o6_world(&projected, true, 2);
        let mut exact = compile_o6_without_pull_waves(&exact_native);
        let kernel = compiled
            .program
            .instant_kernels()
            .iter()
            .find(|kernel| matches!(&kernel.program, KernelProgram::PullWave { .. }) && kernel.actors.len() >= 2)
            .expect("the real shared-wire cell must bind a multi-actor PullWave");
        let wave_actors = kernel.actors.to_vec();
        compiled.test_recorder.instant_kernel_activity = vec![[0; 7]; compiled.program.pistons().len()];
        compiled.set_test_recording(true);
        exact.set_test_recording(true);

        turn_off(&mut compiled, &mut native, CONTROL);
        turn_off(&mut exact, &mut exact_native, CONTROL);
        if interleave_foreign {
            turn_off(&mut compiled, &mut native, foreign_control);
            turn_off(&mut exact, &mut exact_native, foreign_control);
        }
        let initial = compiled
            .future
            .piston_requests
            .iter()
            .map(|request| (request.piston, request.action, request.source))
            .collect::<Vec<_>>();
        assert_eq!(
            initial,
            exact
                .future
                .piston_requests
                .iter()
                .map(|request| (request.piston, request.action, request.source))
                .collect::<Vec<_>>(),
            "the native and optimized graphs begin with the same real FIFO"
        );
        assert_eq!(initial.len(), if interleave_foreign { 2 } else { 1 });
        assert!(wave_actors.contains(&initial[0].0));
        if interleave_foreign {
            assert!(!wave_actors.contains(&initial[1].0), "the queued foreign cell is outside the PullWave");
        }

        let positions: Vec<_> = compiled.program.sites().iter().map(|site| site.position).collect();
        for tick in 0..24 {
            let pop_start = compiled.test_recorder.popped_piston_requests.len();
            let wave_activity_before: Vec<_> = wave_actors
                .iter()
                .map(|actor| compiled.test_recorder.instant_kernel_activity[actor.index()][6])
                .collect();
            compiled.tick().unwrap();
            exact.tick().unwrap();
            native.tick_interpreted();
            exact_native.tick_interpreted();
            assert_backend_state_equal(&compiled, &exact, &format!("resumable FIFO tick={tick}"));
            assert_eq!(
                compiled.test_recorder.popped_piston_requests, exact.test_recorder.popped_piston_requests,
                "actual FIFO actor/action/source pops at tick={tick}"
            );
            if tick == 0 {
                let first_tick_pops = &compiled.test_recorder.popped_piston_requests[pop_start..];
                assert_eq!(first_tick_pops.len(), if interleave_foreign { 3 } else { 2 });
                assert_eq!(&first_tick_pops[..initial.len()], initial.as_slice());
                let wave_commits = wave_actors
                    .iter()
                    .zip(&wave_activity_before)
                    .filter(|(actor, before)| compiled.test_recorder.instant_kernel_activity[actor.index()][6] > **before)
                    .count();
                let child = first_tick_pops.last().unwrap().0;
                assert!(
                    wave_actors.contains(&child),
                    "the final first-tick request is the certified internal child"
                );
                let child_before = wave_activity_before[wave_actors.iter().position(|actor| *actor == child).unwrap()];
                if interleave_foreign {
                    assert_eq!(wave_commits, 1, "the foreign pop cancels the internal child suffix");
                    assert_eq!(
                        compiled.test_recorder.instant_kernel_activity[child.index()][6],
                        child_before,
                        "the internal child remains on native Graph handling after the foreign action"
                    );
                } else {
                    assert!(wave_commits >= 2, "the closed first-tick FIFO executes its internal child");
                    assert_eq!(
                        compiled.test_recorder.instant_kernel_activity[child.index()][6],
                        child_before + 1,
                        "the child request is committed by the PullWave"
                    );
                }
            }
            check_boundary(
                &compiled,
                &native,
                &mut projected,
                &positions,
                &format!("resumable FIFO native tick={tick}"),
            )
            .unwrap();
            check_boundary(
                &exact,
                &exact_native,
                &mut exact_projection,
                &positions,
                &format!("native baseline tick={tick}"),
            )
            .unwrap();
        }
        let compiled_pops = &compiled.test_recorder.popped_piston_requests;
        assert!(compiled_pops.len() >= initial.len());
        assert_eq!(
            &compiled_pops[..initial.len()],
            initial.as_slice(),
            "initial queue order and source are preserved"
        );
        if interleave_foreign {
            assert_eq!(compiled.test_recorder.instant_kernel_activity[initial[1].0.index()][6], 0);
        }
        assert!(
            wave_actors
                .iter()
                .any(|actor| compiled.test_recorder.instant_kernel_activity[actor.index()][6] > 0),
            "an actual bound PullWave request must execute"
        );
        if !interleave_foreign {
            assert!(
                wave_actors
                    .iter()
                    .filter(|actor| compiled.test_recorder.instant_kernel_activity[actor.index()][6] > 0)
                    .count()
                    >= 2,
                "the closed wave must execute its internal child request"
            );
        }
    }
}

#[test]
fn shared_payload_wire_declines_when_turbo_walk_has_an_observer_effect() {
    let mut world = shared_payload_single_wire_world();
    let mut exact_world = shared_payload_single_wire_world();
    let mut native = shared_payload_single_wire_world();
    let wire_position = BlockPos::new(10, 20, 9);
    let observer_position = wire_position.offset(BlockFace::East);
    world.set_block(observer_position.offset(BlockFace::East), Block::Stone {});
    exact_world.set_block(observer_position.offset(BlockFace::East), Block::Stone {});
    native.set_block(observer_position.offset(BlockFace::East), Block::Stone {});
    world.set_block(
        observer_position,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::West,
                powered: false,
            },
        },
    );
    exact_world.set_block(
        observer_position,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::West,
                powered: false,
            },
        },
    );
    native.set_block(
        observer_position,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::West,
                powered: false,
            },
        },
    );

    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
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

    let wire = prepared
        .graph
        .nodes
        .iter()
        .enumerate()
        .find_map(|(index, node)| {
            (node.position == Some(wire_position) && matches!(node.kind, CircuitNodeKind::Wire(_))).then_some(NodeId::new(index as u32))
        })
        .expect("the fixture wire is parsed");
    let CircuitNodeKind::Wire(index) = prepared.graph.nodes[wire.index()].kind else {
        unreachable!()
    };
    let wire_plan = &prepared.graph.wires[index];
    let route = wire_plan.turbo_route.expect("the wire has a native Turbo route");
    let observer = prepared
        .graph
        .nodes
        .iter()
        .enumerate()
        .find_map(|(index, node)| {
            (node.position == Some(observer_position) && matches!(node.kind, CircuitNodeKind::Observer { .. })).then_some(NodeId::new(index as u32))
        })
        .expect("the observer is parsed");
    assert!(
        prepared.graph.routes[route.index()].deliveries.iter().any(|row| {
            row.receiver == Some(CallbackReceiver::Component(observer))
                && row.view == OperandView::Captured
                && matches!(row.kind, DeliveryKind::Update { .. })
        }),
        "the native Turbo route contains the observer's captured update"
    );
    let layer_plan = prepared.graph.plans.instant.layers.as_ref().expect("O6 installs the layer plan");
    assert!(
        layer_plan
            .declines
            .iter()
            .any(|decline| decline.reason == LayerDeclineReason::SignalArrivalUnproven),
        "the PullWave certificate must reject observer side effects: {:?}",
        layer_plan.declines
    );
    let (program, state) = prepared.graph.lower().unwrap();
    assert!(
        program
            .instant_kernels()
            .iter()
            .all(|kernel| { !matches!(&kernel.program, KernelProgram::PullWave { .. }) || !kernel.wires.iter().any(|(owned, _)| *owned == wire) }),
        "the observer-effect wire stays Graph-owned even if unrelated PullWaves are admitted"
    );

    // The Graph-owned route must still deliver the captured observer update and
    // retain its scheduled effects when other regions in the same plot compile.
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    let mut exact = compile_o6_without_pull_waves(&exact_world);
    backend.enable_callback_census();
    turn_off(&mut backend, &mut native, CONTROL);
    turn_off(&mut exact, &mut exact_world, CONTROL);
    let positions: Vec<_> = backend.program.sites().iter().map(|site| site.position).collect();
    for tick in 0..16 {
        backend.tick().unwrap();
        exact.tick().unwrap();
        native.tick_interpreted();
        assert_backend_state_equal(&backend, &exact, &format!("observer-effect fallback tick={tick}"));
        check_boundary(&backend, &native, &mut world, &positions, &format!("observer-effect native tick={tick}")).unwrap();
    }
    assert!(
        backend.callback_census().accepted[0][2] > 0,
        "the native captured Update to the observer is still dispatched"
    );
}

#[test]
fn pull_wave_declines_when_an_internal_actor_has_a_native_owner() {
    let world = shared_payload_single_wire_world();
    let mut backend = compile_o6_world(&world, true, 2);
    let kernel = backend
        .program
        .instant_kernels()
        .iter()
        .find(|kernel| matches!(&kernel.program, KernelProgram::PullWave { .. }) && kernel.actors.len() >= 2)
        .expect("the shared-wire fixture must install a multi-actor PullWave");
    let actors = kernel.actors.to_vec();

    backend.use_block(CONTROL).unwrap();
    let request = backend
        .future
        .piston_requests
        .pop_front()
        .expect("the control must queue a PullWave entry");
    assert!(actors.contains(&request.piston));
    let internal = *actors
        .iter()
        .find(|&&actor| actor != request.piston)
        .expect("the wave must contain an internal actor besides its entry");
    let pending_before = backend.future.pending.clone();
    let pending_nodes_before = backend.future.pending_nodes.clone();
    let requests_before = backend.future.piston_requests.clone();
    let deferred_before = backend.future.deferred_pistons.clone();
    let scheduler_before = backend
        .future
        .scheduler
        .iter()
        .map(|(tick, delay, priority)| (*tick, delay, priority))
        .collect::<Vec<_>>();
    let state_before = backend.state.clone();
    let evaluations_before = backend.execution_counts().logical_kernel_evaluations;
    let invocations_before = backend.execution_counts().instant_kernel_invocations;

    for owner_kind in 0..3 {
        match owner_kind {
            0 => backend.native_action_clock_owner[internal.index()] = Some(0),
            1 => backend.active_frame_retained_actors.push(internal),
            _ => backend.logical_bud_owner_by_actor[internal.index()] = Some(0),
        }

        assert!(
            !backend.run_instant_kernel(request).unwrap(),
            "a PullWave must defer to the actor's native owner"
        );
        assert_eq!(
            backend.state, state_before,
            "owner kind {owner_kind}: no actor or component state changes"
        );
        assert_eq!(
            backend.future.pending, pending_before,
            "owner kind {owner_kind}: pending memberships unchanged"
        );
        assert_eq!(
            backend.future.pending_nodes, pending_nodes_before,
            "owner kind {owner_kind}: pending-node counts unchanged"
        );
        assert_eq!(
            backend.future.piston_requests, requests_before,
            "owner kind {owner_kind}: remaining FIFO unchanged"
        );
        assert_eq!(
            backend.future.deferred_pistons, deferred_before,
            "owner kind {owner_kind}: deferred actions unchanged"
        );
        assert_eq!(
            backend
                .future
                .scheduler
                .iter()
                .map(|(tick, delay, priority)| (*tick, delay, priority))
                .collect::<Vec<_>>(),
            scheduler_before,
            "owner kind {owner_kind}: scheduler unchanged"
        );
        assert_eq!(backend.execution_counts().logical_kernel_evaluations, evaluations_before);
        assert_eq!(backend.execution_counts().instant_kernel_invocations, invocations_before);

        match owner_kind {
            0 => backend.native_action_clock_owner[internal.index()] = None,
            1 => {
                backend.active_frame_retained_actors.pop();
            }
            _ => backend.logical_bud_owner_by_actor[internal.index()] = None,
        };
    }
}
