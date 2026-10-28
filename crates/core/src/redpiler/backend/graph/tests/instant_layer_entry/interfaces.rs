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
            assert_eq!(
                counts.instant_kernel_invocations, 0,
                "powered revalidation keeps the queued request native"
            );
        } else {
            assert!(
                counts.instant_kernel_invocations > 0,
                "the unpowered singleton request executes its compiled route"
            );
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

#[test]
fn event_netlist_shared_payload_and_foreign_cell_keep_distinct_owners() {
    use crate::redpiler::analysis::instant::event_netlist::{callback_sources, candidate_boundary, cut_unsupported_resources, discover};
    let world = shared_payload_with_foreign_fifo_cell_world();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions::parse("-O6").unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::event_domain_research_pipeline(),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let (cell_index, cell) = catalog
        .cells
        .iter()
        .enumerate()
        .find(|(_, cell)| cell.resources.iter().any(|r| r.owners.len() >= 2))
        .expect("shared physical payload");
    let foreign = catalog
        .cells
        .iter()
        .enumerate()
        .find(|(_, cell)| {
            cell.uses
                .iter()
                .any(|use_| graph.pistons[use_.actor.index()].position == BlockPos::new(13, 20, 13))
        })
        .expect("independent physical cell")
        .0;
    let count = catalog.cells.len() + catalog.corridors.len();
    assert!(discover(graph, catalog, &vec![true; count.saturating_sub(1)], &monitor).is_err());
    let cancelled = TaskMonitor::default();
    cancelled.cancel();
    assert!(discover(graph, catalog, &vec![true; count], &cancelled).is_err());
    let all = discover(graph, catalog, &vec![true; count], &monitor).unwrap();
    assert!(
        all.selected[cell_index] && all.selected[foreign],
        "both supported physical groups should be reached"
    );
    for pin in cell.resources.iter().filter(|r| r.owners.len() >= 2) {
        assert_eq!(all.partition.owners.resources[pin.resource.index()], Some(cell_index));
        for actor in &pin.owners {
            assert_eq!(all.partition.owners.actors[actor.index()], Some(cell_index));
        }
    }
    assert_ne!(all.partition.owner_region[cell_index], all.partition.owner_region[foreign]);
    let region = all.partition.owner_region[cell_index];
    let boundary = candidate_boundary(graph, catalog, &all, region, &monitor).unwrap();
    let callback_sources = callback_sources(graph, &boundary, &monitor).unwrap();
    assert!(
        boundary
            .scan
            .ports
            .iter()
            .any(|port| port.direction == BoundaryDirection::Output && matches!(port.kind, BoundaryPortKind::Callback { .. })),
        "physical boundary must contain callback rows"
    );
    assert!(
        callback_sources.is_empty(),
        "receiverless and inert fixture callbacks should not become notifications"
    );
    for source in &callback_sources {
        assert_eq!(boundary.owners.nodes[source.node.index()], Some(0));
        assert!(
            source
                .occurrences
                .iter()
                .all(|occurrence| graph.routes[occurrence.route.index()].source == source.node)
        );
    }
    let mut wire_native = vec![true; count];
    wire_native[catalog.cells.len()..].fill(false);
    let separated = discover(graph, catalog, &wire_native, &monitor).unwrap();
    let exposed = candidate_boundary(graph, catalog, &separated, separated.partition.owner_region[cell_index], &monitor).unwrap();
    assert!(boundary.groups.iter().any(|group| cell.resources.iter().any(|pin| {
        pin.owners.len() >= 2 && group.resources.contains(&pin.resource) && pin.owners.iter().all(|actor| group.actors.contains(actor))
    })));
    for use_ in catalog.cells[foreign].uses.iter() {
        assert_eq!(boundary.owners.actors[use_.actor.index()], None, "independent actors remain native");
    }
    for target in &boundary.aggregate_targets {
        let owner = match *target {
            PowerTarget::Input { node, .. } => boundary.owners.nodes[node.index()],
            PowerTarget::Conductor { site, .. } => boundary.owners.sites[site.index()],
        };
        assert_eq!(owner, None, "aggregate targets cross to a native owner");
    }
    assert!(candidate_boundary(graph, catalog, &all, region, &cancelled).is_err());
    let shared = cell.resources.iter().find(|pin| pin.owners.len() >= 2).unwrap();
    let (contact_program, _) = crate::redpiler::analysis::instant::layers::logical::compile_candidate_state_computation(
        graph,
        &[],
        &[(shared.resource, shared.source)],
        &boundary.owners,
        &monitor,
    )
    .unwrap();
    for &contact in graph.resources[shared.resource.index()].contacts.iter() {
        assert!(contact_program.outputs.iter().any(|output| matches!(output.target,
            SettledTarget::ResourceState { resource, source } if resource == shared.resource && source == contact)));
    }
    let mut incomplete = boundary.owners.clone();
    incomplete.actors[shared.owners[0].index()] = None;
    let error = crate::redpiler::analysis::instant::layers::logical::compile_global_boundary_computation(
        graph,
        &[],
        &[(shared.resource, shared.source)],
        &[],
        &incomplete,
        &monitor,
    )
    .unwrap_err();
    assert!(error.contains("actor outside its owner"), "incomplete shared group must decline: {error}");
    assert!(
        exposed.direct_resource_reads.iter().any(|(resource, _)| *resource == shared.resource),
        "the fixture must expose the shared resource at its candidate boundary"
    );
    let cut_result = cut_unsupported_resources(graph, catalog, &wire_native, &monitor).unwrap();
    assert!(
        cut_result.candidates.selected[cell_index],
        "representable shared ownership remains a single candidate"
    );
    assert!(cut_result.candidates.selected[foreign], "unrelated circuit remains eligible");
    let mut eligible = vec![true; count];
    eligible[cell_index] = false;
    let cut = discover(graph, catalog, &eligible, &monitor).unwrap();
    assert!(!cut.selected[cell_index], "unsupported shared group stays native as a unit");
    assert!(cut.selected[foreign], "an unrelated supported group remains reachable");
    for pin in cell.resources.iter().filter(|r| r.owners.len() >= 2) {
        assert_eq!(cut.partition.owners.resources[pin.resource.index()], Some(cell_index));
        for actor in &pin.owners {
            assert_eq!(cut.partition.owners.actors[actor.index()], Some(cell_index));
        }
    }
}

#[test]
fn event_netlist_external_power_read_keeps_native_source_owner() {
    use crate::redpiler::analysis::instant::event_netlist::{PortRole, discover};
    let world = direct_cell_world();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions::parse("-O6").unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::event_domain_research_pipeline(),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let candidates = discover(graph, catalog, &vec![true; catalog.cells.len() + catalog.corridors.len()], &monitor).unwrap();
    let reads = candidates
        .ports
        .iter()
        .filter_map(|port| {
            if port.role != PortRole::Read {
                return None;
            }
            let BoundaryPortKind::Power { link } = catalog.ports[port.row as usize].kind else {
                return None;
            };
            let site = graph.power_links[link as usize].from;
            (graph.sites[site.index()].position == CONTROL).then_some(site)
        })
        .collect::<Vec<_>>();
    assert!(!reads.is_empty(), "source lever must remain a typed external read");
    for site in reads {
        assert_eq!(
            candidates.partition.owners.sites[site.index()],
            None,
            "reading a native source does not claim ownership"
        );
    }
}

#[test]
fn event_netlist_two_wire_corridor_retains_source_actor_and_guards() {
    use crate::redpiler::analysis::instant::event_netlist::{CandidateFrame, WireStrengthChange, candidate_boundary, corridor_events, discover};
    use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};
    let mut world = wire_controlled_cell_world();
    let middle_wire = CONTROL.offset(BlockFace::West);
    let first_wire = middle_wire.offset(BlockFace::West);
    let lever = first_wire.offset(BlockFace::West);
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(first_wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(lever, world.get_block(middle_wire));
    world.set_block(
        first_wire,
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
    world.set_block(
        middle_wire,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                14,
            ),
        },
    );
    let Block::RedstoneWire { mut wire } = world.get_block(CONTROL) else {
        unreachable!()
    };
    wire.power = 13;
    world.set_block(CONTROL, Block::RedstoneWire { wire });
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions::parse("-O6").unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::event_domain_research_pipeline(),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let candidates = discover(graph, catalog, &vec![true; catalog.cells.len() + catalog.corridors.len()], &monitor).unwrap();
    let actor = graph.pistons.iter().position(|piston| piston.position == BASE).unwrap();
    let owner = candidates.partition.owners.actors[actor].unwrap();
    let region = candidates.partition.owner_region[owner];
    let events = corridor_events(graph, catalog, &candidates, region, &monitor).unwrap();
    let (probe_program, probe_state) = graph.clone().lower().unwrap();
    let mut probe = GraphBackend::new_unactivated(probe_program, probe_state, 0).unwrap();
    probe.use_block(lever).unwrap();
    let native_source = probe
        .future
        .piston_requests
        .iter()
        .find(|request| request.piston == PistonId::new(actor as u32))
        .and_then(|request| request.source)
        .expect("native actor request must retain its source route");
    let event = events
        .iter()
        .find(|event| {
            event.actor == PistonId::new(actor as u32)
                && event.entry_wire != event.source_wire
                && !event.links.is_empty()
                && event.notification.route == native_source
                && graph.nodes[graph.routes[event.entry.route.index()].source.index()].position == Some(lever)
        })
        .expect("the original lever occurrence must reach the piston through owned dust");
    assert_ne!(event.entry_wire, event.source_wire);
    assert!(!event.links.is_empty());
    assert!(
        catalog.cells[event.cell as usize]
            .uses
            .iter()
            .any(|use_| use_.actor == event.actor && use_.activations.contains(&event.notification))
    );
    let transfer = catalog.cells[event.cell as usize]
        .uses
        .iter()
        .find(|use_| use_.actor == event.actor)
        .unwrap()
        .retraction
        .transfer
        .unwrap();
    for &link in &event.links {
        assert!(
            graph.power_links.get(link as usize).is_some(),
            "original link and guard must remain addressable"
        );
    }
    let boundary = candidate_boundary(graph, catalog, &candidates, region, &monitor).unwrap();
    let target = PowerTarget::Input {
        node: event.source_wire,
        port: InputPort::Main,
    };
    let (circuit, _) = crate::redpiler::analysis::instant::layers::logical::compile_candidate_live_computation(
        graph,
        &[target],
        &[(transfer.resource, transfer.source)],
        &boundary.owners,
        &monitor,
    )
    .unwrap();
    let mut frame = CandidateFrame::new_live_states(graph, &boundary.owners, &circuit).unwrap();
    let (program, state) = graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    frame
        .evaluate(|site, port| backend.site_power(site, port), |source| backend.sample_settled_input(source))
        .unwrap();
    let before = frame.outputs()[0];
    assert_eq!(before, backend.wire_state(event.source_wire).power);
    backend.use_block(lever).unwrap();
    frame
        .evaluate(|site, port| backend.site_power(site, port), |source| backend.sample_settled_input(source))
        .unwrap();
    let after = frame.outputs()[0];
    assert_eq!(after, backend.wire_state(event.source_wire).power);
    assert_ne!(before, after, "the physical source must change the exit wire");
    let change = WireStrengthChange {
        node: event.source_wire,
        before,
        after,
    };
    let captured = graph
        .sites
        .iter()
        .enumerate()
        .map(|(index, _)| backend.observe_site(SiteId::new(index as u32)))
        .collect::<Vec<_>>();
    let event = events
        .iter()
        .find(|candidate| {
            candidate.actor == event.actor
                && candidate.entry == event.entry
                && candidate.source_wire == event.source_wire
                && candidate.notification.route == native_source
                && candidate
                    .activates(graph, candidate.entry, change, Some(&captured), |guard, view| {
                        backend.guard_matches(guard, view)
                    })
                    .unwrap_or(false)
        })
        .expect("native source route must contain an enabled captured actor occurrence");
    if graph.routes[event.notification.route.index()].deliveries[event.notification.ordinal as usize].view == OperandView::Captured {
        assert!(
            event.activates(graph, event.entry, change, None, |_, _| true).is_err(),
            "missing captured data must decline"
        );
    }
    assert!(
        event
            .activates(graph, event.entry, change, Some(&captured), |guard, view| backend
                .guard_matches(guard, view))
            .unwrap()
    );
    assert!(
        !event
            .activates(
                graph,
                event.entry,
                WireStrengthChange {
                    node: event.source_wire,
                    before,
                    after: before
                },
                Some(&captured),
                |_, _| true
            )
            .unwrap(),
        "unchanged exit emits no actor activation"
    );
    assert!(
        event
            .activates(
                graph,
                event.entry,
                WireStrengthChange {
                    node: event.entry_wire,
                    before,
                    after
                },
                Some(&captured),
                |_, _| true
            )
            .is_err(),
        "entry changes must not masquerade as exit changes"
    );
    assert!(
        !event.activates(graph, event.entry, change, Some(&captured), |_, _| false).unwrap(),
        "a blocked original path guard emits no activation"
    );
    assert!(
        backend
            .future
            .piston_requests
            .iter()
            .any(|request| request.piston == event.actor && request.source == Some(event.notification.route)),
        "native Graph must queue the same actor from the same wire route"
    );
    let accepted = event
        .transition(
            graph,
            catalog,
            event.entry,
            change,
            Some(&captured),
            |guard, view| backend.guard_matches(guard, view),
            |source| match source {
                CellRead::Contribution(link) => {
                    backend.guard_matches(link.guard, None) && backend.site_power(link.from, link.from_port).saturating_sub(link.attenuation) > 0
                }
                CellRead::State(test) => backend.sample_settled_input(LayerInputSource::State(test)).unwrap(),
                CellRead::Activation(_) => unreachable!(),
            },
        )
        .unwrap()
        .expect("qualified corridor event must request the actual token transfer");
    assert_eq!(accepted, transfer);
    let (virtual_program, virtual_state) = graph.clone().lower().unwrap();
    let mut virtual_backend = GraphBackend::new_unactivated(virtual_program, virtual_state, 0).unwrap();
    virtual_backend.set_resource_contact(transfer.resource, transfer.destination, false);
    frame
        .evaluate(
            |site, port| virtual_backend.site_power(site, port),
            |source| virtual_backend.sample_settled_input(source),
        )
        .unwrap();
    let near_output = circuit
        .outputs
        .iter()
        .position(|output| {
            matches!(output.target,
        SettledTarget::ResourceState { resource, source } if resource == transfer.resource && source == transfer.destination)
        })
        .unwrap();
    assert_eq!(
        frame.outputs()[near_output],
        1,
        "the resource write must remain visible to the following solve"
    );
    backend.drain_piston_actions().unwrap();
    let actual = backend.state.resources[transfer.resource.index()];
    let modeled = virtual_backend.state.resources[transfer.resource.index()];
    assert_eq!(
        (actual.at, actual.enabled),
        (modeled.at, modeled.enabled),
        "the compiled token transition must match native acceptance"
    );
    let (global_live, _) = crate::redpiler::analysis::instant::layers::logical::compile_global_live_computation(
        graph,
        &[target],
        &[(transfer.resource, transfer.source)],
        &[],
        &boundary.owners,
        &monitor,
    )
    .unwrap();
    let mut global_frame = CandidateFrame::new_live_states(graph, &boundary.owners, &global_live).unwrap();
    global_frame
        .evaluate(
            |site, port| virtual_backend.site_power(site, port),
            |source| virtual_backend.sample_settled_input(source),
        )
        .unwrap();
    let global_near = global_live
        .outputs
        .iter()
        .position(|output| {
            matches!(output.target,
        SettledTarget::ResourceState { resource, source } if resource == transfer.resource && source == transfer.destination)
        })
        .unwrap();
    assert_eq!(
        global_frame.outputs()[global_near],
        1,
        "global read-cone lowering must retain live resource state"
    );
    let cancelled = TaskMonitor::default();
    cancelled.cancel();
    assert!(corridor_events(graph, catalog, &candidates, region, &cancelled).is_err());
    let covered = crate::redpiler::analysis::instant::event_netlist::cut_uncovered_callbacks(
        graph,
        catalog,
        &vec![true; catalog.cells.len() + catalog.corridors.len()],
        &monitor,
    )
    .unwrap();
    assert!(covered.rounds >= 1);
    for (owner, occurrence) in &covered.cut_owners {
        assert!(!covered.resources.candidates.selected[*owner]);
        assert!(
            catalog
                .ports
                .iter()
                .any(|port| matches!(port.kind, BoundaryPortKind::Callback { occurrence: value } if value == *occurrence))
        );
    }
    for event in &covered.corridor_events {
        assert!(covered.resources.candidates.selected[event.cell as usize]);
    }
    assert!(
        crate::redpiler::analysis::instant::event_netlist::cut_uncovered_callbacks(
            graph,
            catalog,
            &vec![true; catalog.cells.len() + catalog.corridors.len()],
            &cancelled,
        )
        .is_err()
    );
}

#[test]
fn event_netlist_corridor_uses_notification_wire_not_changed_entry() {
    use crate::redpiler::analysis::instant::event_netlist::{WireStrengthChange, corridor_events, discover};
    use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};
    let mut world = wire_controlled_cell_world();
    let middle = CONTROL.offset(BlockFace::West);
    let first = middle.offset(BlockFace::West);
    let lever = first.offset(BlockFace::West);
    let second = middle.offset(BlockFace::North);
    for position in [lever, first, second] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    let powered_lever = world.get_block(middle);
    world.set_block(lever, powered_lever);
    world.set_block(second, powered_lever);
    for (position, power) in [(first, 15), (middle, 15), (CONTROL, 14)] {
        world.set_block(
            position,
            Block::RedstoneWire {
                wire: RedstoneWire::new(
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    RedstoneWireSide::Side,
                    power,
                ),
            },
        );
    }
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions::parse("-O6").unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::event_domain_research_pipeline(),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let candidates = discover(graph, catalog, &vec![true; catalog.cells.len() + catalog.corridors.len()], &monitor).unwrap();
    let actor = graph.pistons.iter().position(|piston| piston.position == BASE).unwrap();
    let owner = candidates.partition.owners.actors[actor].unwrap();
    let events = corridor_events(graph, catalog, &candidates, candidates.partition.owner_region[owner], &monitor).unwrap();
    let event = events
        .iter()
        .find(|event| {
            event.actor.index() == actor
                && graph.nodes[event.entry_wire.index()].position == Some(first)
                && graph.nodes[event.source_wire.index()].position == Some(CONTROL)
                && graph.nodes[graph.routes[event.entry.route.index()].source.index()].position == Some(lever)
        })
        .expect("lever must have a source-qualified corridor path to the actor");
    let (program, state) = graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    let entry_before = backend.wire_state(event.entry_wire).power;
    let source_before = backend.wire_state(event.source_wire).power;
    backend.use_block(lever).unwrap();
    let entry_after = backend.wire_state(event.entry_wire).power;
    let source_after = backend.wire_state(event.source_wire).power;
    assert_ne!(entry_before, entry_after, "the changed source must change the entry dust");
    assert_eq!(source_before, source_after, "the independent source keeps exit dust powered");
    let change = WireStrengthChange {
        node: event.source_wire,
        before: source_before,
        after: source_after,
    };
    assert!(!event.activates(graph, event.entry, change, None, |_, _| true).unwrap());
    let wrong_wire = WireStrengthChange {
        node: event.entry_wire,
        before: entry_before,
        after: entry_after,
    };
    assert!(event.activates(graph, event.entry, wrong_wire, None, |_, _| true).is_err());
}

#[test]
fn event_netlist_grows_only_directly_reached_comparator() {
    use crate::redpiler::analysis::instant::event_netlist::{actor_ingress, candidate_boundary, discover, grow_comparators};
    use mchprs_blocks::blocks::{ComparatorMode, RedstoneComparator};
    let mut world = direct_cell_world();
    let reached = BASE.offset(BlockFace::North);
    let unrelated = BlockPos::new(13, 20, 13);
    for position in [reached, unrelated] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::North, ComparatorMode::Compare, false),
            },
        );
        world.set_block_entity(position, BlockEntity::Comparator { output_strength: 0 });
    }
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions::parse("-O6").unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::event_domain_research_pipeline(),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let candidates = discover(graph, catalog, &vec![true; catalog.cells.len() + catalog.corridors.len()], &monitor).unwrap();
    let actor = graph.pistons.iter().position(|piston| piston.position == BASE).unwrap();
    let owner = candidates.partition.owners.actors[actor].unwrap();
    let region = candidates.partition.owner_region[owner];
    let mut boundary = candidate_boundary(graph, catalog, &candidates, region, &monitor).unwrap();
    let (ingress, unresolved) = actor_ingress(graph, catalog, &boundary, &monitor).unwrap();
    assert!(unresolved > 0, "dynamic contact deliveries must remain an explicit native boundary");
    assert!(
        ingress.iter().any(|entry| entry.actor == PistonId::new(actor as u32)
            && graph.nodes[graph.routes[entry.occurrence.route.index()].source.index()].position == Some(CONTROL)),
        "the direct lever callback must retain its actor and original source occurrence"
    );
    let cancelled = TaskMonitor::default();
    cancelled.cancel();
    assert!(grow_comparators(graph, &candidates, &mut boundary, &cancelled).is_err());
    let grown = grow_comparators(graph, &candidates, &mut boundary, &monitor).unwrap();
    let reached_node = graph.nodes.iter().position(|node| node.position == Some(reached)).unwrap();
    let unrelated_node = graph.nodes.iter().position(|node| node.position == Some(unrelated)).unwrap();
    assert!(grown.contains(&NodeId::new(reached_node as u32)));
    assert_eq!(boundary.owners.nodes[reached_node], Some(0));
    assert_eq!(boundary.owners.nodes[unrelated_node], None, "independent comparator remains native");
}

#[test]
fn event_netlist_native_wire_cut_preserves_original_power_link() {
    use crate::redpiler::analysis::instant::event_netlist::{
        CandidateFrame, CutSource, SourceCut, SourceOwner, candidate_boundary, discover, source_cuts,
    };
    let world = wire_controlled_cell_world();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions::parse("-O6").unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::event_domain_research_pipeline(),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let mut eligible = vec![true; catalog.cells.len() + catalog.corridors.len()];
    eligible[catalog.cells.len()..].fill(false);
    let candidates = discover(graph, catalog, &eligible, &monitor).unwrap();
    let cuts: Vec<SourceCut> = source_cuts(graph, catalog, &candidates, &monitor).unwrap();
    let wires = cuts
        .iter()
        .filter_map(|cut| match cut.source {
            CutSource::Wire { node, link } if cut.owner == SourceOwner::Native => Some((cut, node, link)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!wires.is_empty(), "native wire source must remain an explicit live cut");
    let &(cut, _, link) = wires
        .iter()
        .find(|(_, _, link)| matches!(graph.power_links[*link as usize].to, PowerTarget::Input { .. }))
        .expect("native wire must feed a component input");
    let boundary = candidate_boundary(graph, catalog, &candidates, cut.region, &monitor).unwrap();
    let edge = graph.power_links[link as usize];
    let (circuit, _) =
        crate::redpiler::analysis::instant::layers::logical::compile_candidate_computation(graph, &[edge.to], &boundary.owners, &monitor).unwrap();
    assert!(
        circuit.program.inputs.iter().any(|input| matches!(input.source,
        LayerInputSource::SourceAbove { site, .. } if site == edge.from)),
        "candidate lowering must read the original native wire rather than claim its state"
    );
    let pin = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.resources.iter())
        .find(|pin| {
            boundary.owners.resources[pin.resource.index()].is_some()
                && pin.owners.len() == 1
                && graph.resources[pin.resource.index()].contacts.len() == 2
        })
        .unwrap();
    let (with_state, _) = crate::redpiler::analysis::instant::layers::logical::compile_candidate_state_computation(
        graph,
        &[edge.to],
        &[(pin.resource, pin.source)],
        &boundary.owners,
        &monitor,
    )
    .unwrap();
    assert!(with_state.outputs.iter().any(|output| matches!(output.target,
        SettledTarget::ResourceState { resource, source } if resource == pin.resource && source == pin.source)));
    CandidateFrame::new(graph, &boundary.owners, &with_state).unwrap();
    let (expanded, _) = crate::redpiler::analysis::instant::layers::logical::compile_candidate_with_native_wires(
        graph,
        &[edge.to],
        &[(pin.resource, pin.source)],
        &boundary.owners,
        &monitor,
    )
    .unwrap();
    let (live, _) = crate::redpiler::analysis::instant::layers::logical::compile_candidate_live_computation(
        graph,
        &[edge.to],
        &[(pin.resource, pin.source)],
        &boundary.owners,
        &monitor,
    )
    .unwrap();
    assert_eq!(boundary.owners.sites[edge.from.index()], None, "expanded native wire remains Graph-owned");
    let mut conflicting = boundary.owners.clone();
    conflicting.sites[edge.from.index()] = Some(0);
    assert!(
        CandidateFrame::new(graph, &conflicting, &circuit).is_err(),
        "an internally owned producer cannot be silently treated as a native input"
    );
    let (program, state) = graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    let mut live_frame = CandidateFrame::new_live_states(graph, &boundary.owners, &live).unwrap();
    let destination = graph.resources[pin.resource.index()]
        .contacts
        .iter()
        .copied()
        .find(|contact| *contact != pin.source)
        .unwrap();
    let source_output = live
        .outputs
        .iter()
        .position(
            |output| matches!(output.target, SettledTarget::ResourceState { resource, source } if resource == pin.resource && source == pin.source),
        )
        .unwrap();
    let destination_output = live
        .outputs
        .iter()
        .position(
            |output| matches!(output.target, SettledTarget::ResourceState { resource, source } if resource == pin.resource && source == destination),
        )
        .unwrap();
    live_frame
        .evaluate(|site, port| backend.site_power(site, port), |source| backend.sample_settled_input(source))
        .unwrap();
    assert_eq!((live_frame.outputs()[source_output], live_frame.outputs()[destination_output]), (1, 0));
    backend.set_resource_contact(pin.resource, destination, true);
    live_frame
        .evaluate(|site, port| backend.site_power(site, port), |source| backend.sample_settled_input(source))
        .unwrap();
    assert_eq!(
        (live_frame.outputs()[source_output], live_frame.outputs()[destination_output]),
        (0, 1),
        "solving must retain the applied virtual resource transition"
    );
    backend.set_resource_contact(pin.resource, pin.source, true);
    live_frame
        .evaluate(|site, port| backend.site_power(site, port), |source| backend.sample_settled_input(source))
        .unwrap();
    assert_eq!((live_frame.outputs()[source_output], live_frame.outputs()[destination_output]), (1, 0));
    let mut evaluation = CandidateFrame::new(graph, &boundary.owners, &circuit).unwrap();
    let mut expanded_evaluation = CandidateFrame::new(graph, &boundary.owners, &expanded).unwrap();
    let PowerTarget::Input { node, port } = edge.to else { unreachable!() };
    for _ in 0..4 {
        backend.use_block(CONTROL.offset(BlockFace::West)).unwrap();
        for _ in 0..7 {
            evaluation
                .evaluate(|site, port| backend.site_power(site, port), |source| backend.sample_settled_input(source))
                .unwrap();
            let expected = backend.input_strength(node, port);
            let actual = evaluation.outputs()[0];
            assert_eq!(actual > 0, expected > 0, "candidate cut must follow native wire changes");
            expanded_evaluation
                .evaluate(|site, port| backend.site_power(site, port), |source| backend.sample_settled_input(source))
                .unwrap();
            assert_eq!(
                expanded_evaluation.outputs()[0] > 0,
                expected > 0,
                "read-only expanded native wire must follow its independent Graph source"
            );
            backend.tick().unwrap();
        }
    }
    for (cut, node, link) in wires {
        assert_eq!(catalog.ports[cut.row as usize].kind, BoundaryPortKind::Power { link });
        let edge = graph.power_links[link as usize];
        assert!(matches!(graph.sites[edge.from.index()].source,SiteSource::Component(source) if source == node));
        assert_eq!(
            candidates.partition.owners.nodes[node.index()].map(|owner| candidates.selected[owner]),
            Some(false)
        );
    }
    let cancelled = TaskMonitor::default();
    cancelled.cancel();
    assert!(source_cuts(graph, catalog, &candidates, &cancelled).is_err());
}

#[test]
fn event_netlist_common_native_wire_does_not_merge_consumers() {
    use crate::redpiler::analysis::instant::event_netlist::{CutSource, SourceOwner, discover, source_cuts};
    let mut world = wire_controlled_cell_world();
    let offset = BlockPos::new(0, 0, 5);
    for position in [
        BASE,
        BASE.offset(BlockFace::East),
        BASE.offset(BlockFace::East).offset(BlockFace::East),
        BASE.offset(BlockFace::Top),
        BASE.offset(BlockFace::Top).offset(BlockFace::Top),
        CONTROL,
        CONTROL.offset(BlockFace::Bottom),
    ] {
        world.set_block(position + offset, world.get_block(position));
    }
    for z in 9..13 {
        let wire = BlockPos::new(7, 20, z);
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
    }
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions::parse("-O6").unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::event_domain_research_pipeline(),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let shared = catalog
        .cells
        .iter()
        .position(|cell| cell.uses.iter().any(|use_| graph.pistons[use_.actor.index()].position == BASE))
        .unwrap();
    let other = catalog
        .cells
        .iter()
        .position(|cell| cell.uses.iter().any(|use_| graph.pistons[use_.actor.index()].position == BASE + offset))
        .unwrap();
    let mut eligible = vec![true; catalog.cells.len() + catalog.corridors.len()];
    eligible[catalog.cells.len()..].fill(false);
    let candidates = discover(graph, catalog, &eligible, &monitor).unwrap();
    assert!(
        candidates.selected[shared] && candidates.selected[other],
        "both consumers must actually be discovered"
    );
    assert_ne!(
        candidates.partition.owner_region[shared], candidates.partition.owner_region[other],
        "common native wire is a read, not shared ownership"
    );
    let cuts = source_cuts(graph, catalog, &candidates, &monitor).unwrap();
    let native = cuts
        .iter()
        .filter_map(|cut| match cut.source {
            CutSource::Wire { node, .. } if cut.owner == SourceOwner::Native => Some((cut.region, node)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(native.iter().any(|(region, _)| *region == candidates.partition.owner_region[shared]));
    assert!(native.iter().any(|(region, _)| *region == candidates.partition.owner_region[other]));
}

#[test]
fn event_netlist_batched_native_wire_reads_match_live_graph_samples() {
    use crate::redpiler::analysis::instant::layers::compile_native_power_cuts;
    let world = wire_controlled_cell_world();
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let graph = &prepared.graph;
    let actor = graph.pistons.iter().find(|piston| piston.position == BASE).unwrap();
    let mut targets = Vec::new();
    for port in [InputPort::Main, InputPort::Quasi] {
        for threshold in 0..15 {
            targets.push((PowerTarget::Input { node: actor.node, port }, threshold));
        }
    }
    let program = compile_native_power_cuts(graph, &targets, &monitor).unwrap();
    assert!(
        program
            .inputs
            .iter()
            .any(|input| matches!(input.source,LayerInputSource::SourceAbove { site,.. } if graph.sites[site.index()].position == CONTROL)),
        "the wire must remain a live source input"
    );
    let (lowered, state) = graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new_unactivated(lowered, state, 0).unwrap();
    let mut scratch = vec![false; program.expressions.len()];
    for cycle in 0..4 {
        backend.use_block(CONTROL.offset(BlockFace::West)).unwrap();
        for tick in 0..7 {
            program
                .evaluate(&mut scratch, |input| backend.sample_settled_input(input.source).unwrap())
                .unwrap();
            for (index, (_, root)) in program.roots.iter().enumerate() {
                let (PowerTarget::Input { node, port }, threshold) = targets[index] else {
                    unreachable!()
                };
                assert_eq!(
                    scratch[root.0 as usize],
                    backend.input_strength(node, port) > threshold,
                    "live power cycle {cycle} tick {tick} root {index}"
                );
            }
            backend.tick().unwrap();
        }
    }
}

/// Experimental idealized private NOT frame. The only native output is the
/// lamp; owned actuator/reset state is held at its quiescent entry pose.
#[test]
fn event_netlist_idealized_private_not_frame_executes_and_materializes() {
    use crate::redpiler::analysis::instant::layers::compile_native_power_cuts;
    let output = BlockPos::new(10, 20, 9);
    let data = BASE.offset(BlockFace::North);
    let build = || {
        let mut world = direct_cell_world();
        world.set_block(output, Block::RedstoneLamp { lit: true });
        world.set_block(data.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            data,
            Block::Lever {
                lever: Lever {
                    face: LeverFace::Floor,
                    facing: BlockDirection::North,
                    powered: false,
                },
            },
        );
        world
    };
    let world = build();
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let graph = &prepared.graph;
    assert_eq!(graph.pistons.len(), 1, "multi-actor ownership is outside this frame");
    assert_eq!(graph.resources.len(), 1, "shared or unrelated resources remain native");
    let catalog = infer_boundaries(graph, &monitor).unwrap();
    assert_eq!(catalog.cells.len(), 1, "frame declines unrecognized or interacting cells");
    let cell = &catalog.cells[0];
    assert_eq!(cell.uses.len(), 1);
    assert_eq!(cell.resources.len(), 1);
    let gate_use = &cell.uses[0];
    let pin = &cell.resources[0];
    assert_eq!(&*pin.owners, &[gate_use.actor], "resource ownership must be exclusive");
    assert!(gate_use.reset.observer().is_some(), "only the private observer-reset shape is admitted");
    let spec = graph.pistons[gate_use.actor.index()];
    assert!(matches!(spec.lane,PistonLane::SingleResource { far } if far == pin.source));
    let lamp = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(output)).unwrap() as u32);
    let candidates = crate::redpiler::analysis::instant::event_netlist::discover(
        graph,
        &catalog,
        &vec![true; catalog.cells.len() + catalog.corridors.len()],
        &monitor,
    )
    .unwrap();
    let plan: crate::redpiler::analysis::instant::event_netlist::PrivateNot =
        crate::redpiler::analysis::instant::event_netlist::lower_private_not(graph, &catalog, &candidates.partition.owners, lamp, &monitor).unwrap();
    assert_eq!(plan.actor, gate_use.actor);
    assert_eq!(plan.resource, pin.resource);
    assert_eq!(plan.far, pin.source);
    assert_eq!(plan.observer, gate_use.reset.observer().unwrap());
    let far = spec.routes.far_changed.expect("native Far boundary route");
    assert_eq!(plan.route, far);
    let deliveries = graph.routes[far.index()]
        .deliveries
        .iter()
        .enumerate()
        .filter(|(_, delivery)| delivery.receiver == Some(CallbackReceiver::Component(lamp)))
        .map(|(ordinal, delivery)| (ordinal, *delivery))
        .collect::<Vec<_>>();
    assert!(!deliveries.is_empty(), "the physical output must receive an original Far notification");
    assert_eq!(plan.deliveries.len(), deliveries.len());
    assert_eq!(&*plan.activations, &*gate_use.activations);
    assert!(
        deliveries.iter().all(|(_, delivery)| delivery.view == OperandView::Live),
        "captured egress stays native"
    );
    for route in [spec.routes.base_changed, spec.routes.near_changed] {
        assert!(
            graph.routes[route.index()]
                .deliveries
                .iter()
                .all(|delivery| delivery.receiver != Some(CallbackReceiver::Component(lamp))),
            "skipped actuator effects must not reach the native lamp"
        );
    }
    let power = compile_native_power_cuts(
        graph,
        &[
            (
                PowerTarget::Input {
                    node: spec.node,
                    port: InputPort::Main,
                },
                0,
            ),
            (
                PowerTarget::Input {
                    node: spec.node,
                    port: InputPort::Quasi,
                },
                0,
            ),
        ],
        &monitor,
    )
    .unwrap();
    assert_eq!(plan.power, power);
    let (program, state) = graph.clone().lower().unwrap();
    let mut frame = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
    let mut high_input = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
    let mut native = GraphBackend::new_unactivated(program, state, 0).unwrap();
    high_input.use_block(data).unwrap();
    high_input.use_block(CONTROL).unwrap();
    let mut high_values = vec![false; power.expressions.len()];
    power
        .evaluate(&mut high_values, |input| high_input.sample_settled_input(input.source).unwrap())
        .unwrap();
    assert!(
        power.roots.iter().any(|(_, root)| high_values[root.0 as usize]),
        "true data input inhibits the idealized NOT pulse"
    );
    assert!(
        high_input.future.piston_requests.is_empty(),
        "the independent trigger must not queue an inhibited piston"
    );
    for _ in 0..6 {
        high_input.tick().unwrap();
    }
    assert!(high_input.node_state(lamp).powered, "true data input emits no falling output");
    let mut initial = vec![false; power.expressions.len()];
    power
        .evaluate(&mut initial, |input| frame.sample_settled_input(input.source).unwrap())
        .unwrap();
    assert!(
        power.roots.iter().any(|(_, root)| initial[root.0 as usize]),
        "true input produces no NOT pulse"
    );
    frame.use_block(CONTROL).unwrap();
    native.use_block(CONTROL).unwrap();
    assert_eq!(frame.future.piston_requests.len(), 1, "only one source-qualified event is admitted");
    let request = frame.future.piston_requests.front().copied().unwrap();
    assert_eq!(request.piston, gate_use.actor);
    assert_eq!(request.action, PistonAction::Retract);
    assert!(
        gate_use.activations.iter().any(|occurrence| Some(occurrence.route) == request.source),
        "event must retain its source route"
    );
    assert_eq!(frame.state.pistons[gate_use.actor.index()].availability, PistonAvailability::Ready);
    assert!(frame.state.pistons[gate_use.actor.index()].extended && frame.state.pistons[gate_use.actor.index()].head_present);
    assert!(frame.future.deferred_pistons.is_empty(), "pending owned reset remains native");
    let state = frame.state.resources[pin.resource.index()];
    assert!(state.enabled && state.at == pin.source, "quiescent source entry");
    let mut scratch = vec![false; power.expressions.len()];
    power
        .evaluate(&mut scratch, |input| frame.sample_settled_input(input.source).unwrap())
        .unwrap();
    let input_powered = power.roots.iter().any(|(_, root)| scratch[root.0 as usize]);
    let output_event = !input_powered;
    assert!(output_event, "false input produces the idealized NOT pulse");
    frame.future.piston_requests.pop_front();
    let emit = |frame: &mut GraphBackend| {
        for &(ordinal, delivery) in &deliveries {
            frame
                .deliver_callback(
                    far,
                    delivery,
                    None,
                    Some(RouteOccurrence {
                        route: far,
                        ordinal: ordinal as u32,
                    }),
                )
                .unwrap();
        }
    };
    frame.set_resource_contact(pin.resource, pin.source, false).unwrap();
    emit(&mut frame);
    for _ in 0..6 {
        frame.tick().unwrap();
        native.tick().unwrap();
    }
    println!(
        "IDEALIZED_PRIVATE_NOT pulse frame={:?} native={:?}",
        frame.state.components[lamp.index()],
        native.state.components[lamp.index()]
    );
    assert!(
        !frame.node_state(lamp).powered,
        "the false input must publish an observable falling output"
    );
    frame.set_resource_contact(pin.resource, pin.source, true).unwrap();
    emit(&mut frame);
    frame.use_block(CONTROL).unwrap();
    native.use_block(CONTROL).unwrap();
    for _ in 0..24 {
        frame.tick().unwrap();
        native.tick().unwrap();
    }
    assert_eq!(frame.state.resources[pin.resource.index()].at, pin.source);
    assert!(frame.state.resources[pin.resource.index()].enabled);
    assert!(
        frame.future.piston_requests.is_empty() && frame.future.deferred_pistons.is_empty(),
        "frame handoff is quiescent"
    );
    let (ComponentState::Electrical(frame_lamp), ComponentState::Electrical(native_lamp)) =
        (&frame.state.components[lamp.index()], &native.state.components[lamp.index()])
    else {
        panic!("lamp state");
    };
    assert_eq!(frame_lamp.powered, native_lamp.powered, "native lamp settles to the same visible output");
    println!(
        "IDEALIZED_PRIVATE_NOT lamp_internal_strength frame={} native={}",
        frame_lamp.output_strength, native_lamp.output_strength
    );
    assert_eq!(frame_lamp, native_lamp, "native lamp state must materialize exactly at handoff");
    assert_eq!(
        frame.state.pistons[gate_use.actor.index()].extended,
        native.state.pistons[gate_use.actor.index()].extended
    );
    assert_eq!(
        frame.state.pistons[gate_use.actor.index()].head_present,
        native.state.pistons[gate_use.actor.index()].head_present
    );
    let observer = gate_use.reset.observer().unwrap();
    assert_eq!(
        frame.state.components[observer.index()],
        native.state.components[observer.index()],
        "observer state at handoff"
    );
    assert_eq!(
        frame.future.scheduler.iter().collect::<Vec<_>>(),
        native.future.scheduler.iter().collect::<Vec<_>>(),
        "native scheduled work at handoff"
    );
    assert_eq!(frame.execution_counts().piston_actions, 0, "frame must replace the interior actor work");
    assert!(
        native.execution_counts().piston_actions > 0,
        "native reference must exercise real piston work"
    );
    for _ in 0..24 {
        frame.tick().unwrap();
        native.tick().unwrap();
        assert_eq!(
            frame.state.components[lamp.index()],
            native.state.components[lamp.index()],
            "native lamp continuation"
        );
        assert_eq!(
            frame.state.components[observer.index()],
            native.state.components[observer.index()],
            "native observer continuation"
        );
    }
    for repetition in 0..2 {
        frame.use_block(data).unwrap();
        native.use_block(data).unwrap();
        frame.use_block(CONTROL).unwrap();
        native.use_block(CONTROL).unwrap();
        if repetition == 0 {
            assert!(frame.future.piston_requests.is_empty(), "true data inhibits the independent trigger");
            power
                .evaluate(&mut scratch, |input| frame.sample_settled_input(input.source).unwrap())
                .unwrap();
            assert!(power.roots.iter().any(|(_, root)| scratch[root.0 as usize]), "true data sample");
            for _ in 0..6 {
                frame.tick().unwrap();
                native.tick().unwrap();
            }
            assert!(frame.node_state(lamp).powered, "true data emits no falling output");
            frame.use_block(CONTROL).unwrap();
            native.use_block(CONTROL).unwrap();
            for _ in 0..24 {
                frame.tick().unwrap();
                native.tick().unwrap();
            }
            assert_eq!(
                frame.state.components[lamp.index()],
                native.state.components[lamp.index()],
                "true-input handoff"
            );
            continue;
        }
        assert_eq!(frame.future.piston_requests.len(), 1, "one new trigger is required");
        let request = frame.future.piston_requests.pop_front().unwrap();
        assert_eq!(request.piston, gate_use.actor);
        assert!(gate_use.activations.iter().any(|occurrence| Some(occurrence.route) == request.source));
        power
            .evaluate(&mut scratch, |input| frame.sample_settled_input(input.source).unwrap())
            .unwrap();
        assert!(!power.roots.iter().any(|(_, root)| scratch[root.0 as usize]), "NOT input {repetition}");
        frame.set_resource_contact(pin.resource, pin.source, false).unwrap();
        emit(&mut frame);
        for _ in 0..6 {
            frame.tick().unwrap();
            native.tick().unwrap();
        }
        frame.set_resource_contact(pin.resource, pin.source, true).unwrap();
        emit(&mut frame);
        frame.use_block(CONTROL).unwrap();
        native.use_block(CONTROL).unwrap();
        for _ in 0..24 {
            frame.tick().unwrap();
            native.tick().unwrap();
        }
        assert_eq!(
            frame.state.components[lamp.index()],
            native.state.components[lamp.index()],
            "native lamp at frame {repetition}"
        );
        assert_eq!(
            frame.state.components[observer.index()],
            native.state.components[observer.index()],
            "observer at frame {repetition}"
        );
        assert!(
            frame.future.piston_requests.is_empty() && frame.future.deferred_pistons.is_empty(),
            "quiescent frame {repetition}"
        );
    }
    // Continuous experiment: four input changes at six-tick intervals, with
    // no native reset wait between frames. Only the private gate stays owned.
    for index in 0..4 {
        frame.use_block(data).unwrap();
        native.use_block(data).unwrap();
        frame.use_block(CONTROL).unwrap();
        native.use_block(CONTROL).unwrap();
        power
            .evaluate(&mut scratch, |input| frame.sample_settled_input(input.source).unwrap())
            .unwrap();
        let high = power.roots.iter().any(|(_, root)| scratch[root.0 as usize]);
        assert_eq!(high, index % 2 == 0, "changing data input at frame {index}");
        if high {
            assert!(frame.future.piston_requests.is_empty(), "high input must inhibit the private gate");
        } else {
            assert_eq!(frame.future.piston_requests.len(), 1, "one source-qualified trigger");
            let request = frame.future.piston_requests.pop_front().unwrap();
            assert_eq!(request.piston, gate_use.actor);
            assert!(gate_use.activations.iter().any(|occurrence| Some(occurrence.route) == request.source));
            frame.set_resource_contact(pin.resource, pin.source, false).unwrap();
            emit(&mut frame);
        }
        for _ in 0..6 {
            frame.tick().unwrap();
            native.tick().unwrap();
        }
        assert_eq!(frame.node_state(lamp).powered, high, "idealized NOT output at frame {index}");
        if !high {
            frame.set_resource_contact(pin.resource, pin.source, true).unwrap();
            emit(&mut frame);
        }
        frame.use_block(CONTROL).unwrap();
        native.use_block(CONTROL).unwrap();
        assert!(
            frame.future.piston_requests.is_empty() && frame.future.deferred_pistons.is_empty(),
            "owned work at frame {index}"
        );
    }
    for _ in 0..24 {
        frame.tick().unwrap();
        native.tick().unwrap();
    }
    assert_eq!(
        frame.state.components[lamp.index()],
        native.state.components[lamp.index()],
        "lamp after six-tick re-entry"
    );
    assert_eq!(
        frame.state.components[observer.index()],
        native.state.components[observer.index()],
        "observer after six-tick re-entry"
    );
    assert_eq!(
        frame.future.scheduler.iter().collect::<Vec<_>>(),
        native.future.scheduler.iter().collect::<Vec<_>>(),
        "scheduled work after six-tick re-entry"
    );
    println!(
        "IDEALIZED_PRIVATE_NOT frame={:?} native={:?}",
        frame.execution_counts(),
        native.execution_counts()
    );
}

#[test]
fn event_netlist_selected_power_producer_and_consumer_grow_together() {
    use crate::redpiler::analysis::instant::event_netlist::{candidate_boundary, discover};
    let world = shared_payload_single_wire_multiconsumer_world();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = crate::redpiler::CompilerOptions::parse("-O6").unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::event_domain_research_pipeline(),
        &monitor,
    )
    .unwrap();
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let first = catalog
        .cells
        .iter()
        .position(|cell| cell.resources.iter().any(|resource| resource.owners.len() >= 2))
        .unwrap();
    let second = catalog
        .cells
        .iter()
        .position(|cell| {
            cell.uses
                .iter()
                .any(|use_| graph.pistons[use_.actor.index()].position == BlockPos::new(9, 20, 9))
        })
        .unwrap();
    assert!(
        catalog.ports.iter().any(|a| a.owner == BoundaryOwner::Cell(first as u32)
            && a.direction == BoundaryDirection::Output
            && matches!(a.kind, BoundaryPortKind::Power { .. })
            && catalog
                .ports
                .iter()
                .any(|b| b.owner == BoundaryOwner::Cell(second as u32) && b.direction == BoundaryDirection::Input && a.kind == b.kind)),
        "fixture needs an original producer-to-consumer power link"
    );
    let candidates = discover(graph, catalog, &vec![true; catalog.cells.len() + catalog.corridors.len()], &monitor).unwrap();
    assert!(candidates.selected[first] && candidates.selected[second]);
    assert!(
        candidates
            .edges
            .iter()
            .any(|edge| edge.source == first && edge.target == second && matches!(edge.kind, BoundaryPortKind::Power { .. })),
        "the reached producer must change the consumer's power source"
    );
    assert_eq!(candidates.partition.owner_region[first], candidates.partition.owner_region[second]);
    let boundary = candidate_boundary(graph, catalog, &candidates, candidates.partition.owner_region[first], &monitor).unwrap();
    let internal = candidates
        .edges
        .iter()
        .find(|edge| edge.source == first && edge.target == second && matches!(edge.kind, BoundaryPortKind::Power { .. }))
        .unwrap();
    assert!(
        !boundary
            .scan
            .ports
            .iter()
            .any(|port| port.direction == BoundaryDirection::Output && port.kind == internal.kind),
        "a selected producer-to-consumer edge is internal to the whole-region cut"
    );
}

/// Test-only runtime: unsupported entry stays in the original Graph FIFO.
struct AutomaticPrivateNot {
    plan: crate::redpiler::analysis::instant::event_netlist::PrivateNot,
    remaining: u8,
    executions: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum PrivateNotTrigger {
    Started,
    Native,
    Busy,
}

impl AutomaticPrivateNot {
    fn trigger(&mut self, backend: &mut GraphBackend, position: BlockPos) -> Result<PrivateNotTrigger, GraphExecutionError> {
        // The caller must retain this input until the private frame completes.
        if self.remaining != 0 {
            return Ok(PrivateNotTrigger::Busy);
        }
        backend.use_block(position)?;
        if backend.future.piston_requests.len() != 1 || !backend.future.deferred_pistons.is_empty() {
            return Ok(PrivateNotTrigger::Native);
        }
        let request = backend.future.piston_requests.front().copied().unwrap();
        let state = backend.state.pistons[self.plan.actor.index()];
        let resource = backend.state.resources[self.plan.resource.index()];
        let source_matches = request.source.is_some_and(|route| self.plan.ingress_routes.binary_search(&route).is_ok());
        if request.piston != self.plan.actor
            || request.action != PistonAction::Retract
            || !source_matches
            || !state.extended
            || !state.head_present
            || state.availability != PistonAvailability::Ready
            || !resource.enabled
            || resource.at != self.plan.far
            || backend.is_pending(self.plan.observer)
        {
            return Ok(PrivateNotTrigger::Native);
        }
        let inputs = match self
            .plan
            .power
            .inputs
            .iter()
            .map(|input| backend.sample_settled_input(input.source))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(inputs) => inputs,
            Err(_) => return Ok(PrivateNotTrigger::Native),
        };
        let mut scratch = vec![false; self.plan.power.expressions.len()];
        if self
            .plan
            .power
            .evaluate(&mut scratch, |input| {
                let index = self.plan.power.inputs.iter().position(|slot| *slot == input).expect("validated input");
                inputs[index]
            })
            .is_err()
        {
            return Ok(PrivateNotTrigger::Native);
        }
        if self.plan.power.roots.iter().any(|(_, root)| scratch[root.0 as usize]) {
            return Ok(PrivateNotTrigger::Native);
        }
        backend.future.piston_requests.pop_front();
        backend.set_resource_contact(self.plan.resource, self.plan.near, false).unwrap();
        self.emit(backend)?;
        self.remaining = 6;
        self.executions += 1;
        Ok(PrivateNotTrigger::Started)
    }

    fn tick(&mut self, backend: &mut GraphBackend) -> Result<(), GraphExecutionError> {
        backend.tick()?;
        if self.remaining > 0 {
            self.remaining -= 1;
            if self.remaining == 0 {
                backend.set_resource_contact(self.plan.resource, self.plan.far, true).unwrap();
                self.emit(backend)?;
            }
        }
        Ok(())
    }

    fn emit(&self, backend: &mut GraphBackend) -> Result<(), GraphExecutionError> {
        for &(ordinal, delivery) in &self.plan.deliveries {
            backend.deliver_callback(
                self.plan.route,
                delivery,
                None,
                Some(RouteOccurrence {
                    route: self.plan.route,
                    ordinal,
                }),
            )?;
        }
        Ok(())
    }
}

#[test]
fn event_netlist_automatic_private_not_executes_six_tick_frames() {
    use crate::redpiler::analysis::instant::event_netlist::{discover, lower_private_not};
    let output = BlockPos::new(10, 20, 9);
    let data = BASE.offset(BlockFace::North);
    let mut world = direct_cell_world();
    world.set_block(output, Block::RedstoneLamp { lit: true });
    world.set_block(data.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        data,
        Block::Lever {
            lever: Lever {
                face: LeverFace::Floor,
                facing: BlockDirection::North,
                powered: false,
            },
        },
    );
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let graph = &prepared.graph;
    let catalog = infer_boundaries(graph, &monitor).unwrap();
    let candidates = discover(graph, &catalog, &vec![true; catalog.cells.len() + catalog.corridors.len()], &monitor).unwrap();
    let lamp = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(output)).unwrap() as u32);
    let plan = lower_private_not(graph, &catalog, &candidates.partition.owners, lamp, &monitor).expect("isolated private gate admission");
    let trigger_node = graph.nodes.iter().position(|node| node.position == Some(CONTROL)).unwrap();
    let trigger_route = graph.nodes[trigger_node].output_route.unwrap();
    assert!(plan.ingress_routes.contains(&trigger_route));
    let ambiguous_event = plan.activations.iter().find(|event| event.route == trigger_route).copied().unwrap();
    let mut ambiguous = graph.clone();
    ambiguous.routes[trigger_route.index()].deliveries[ambiguous_event.ordinal as usize].view = OperandView::Captured;
    let cut = lower_private_not(&ambiguous, &catalog, &candidates.partition.owners, lamp, &monitor);
    assert!(
        !cut.as_ref().is_ok_and(|plan| plan.ingress_routes.contains(&trigger_route)),
        "captured or ambiguous source must remain native"
    );
    let (program, state) = graph.clone().lower().unwrap();
    let mut frame = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
    let mut native = GraphBackend::new_unactivated(program, state, 0).unwrap();
    let mut runtime = AutomaticPrivateNot {
        plan,
        remaining: 0,
        executions: 0,
    };
    let mut timing_mismatches = 0usize;
    for index in 0..4 {
        if index != 0 {
            frame.use_block(data).unwrap();
            native.use_block(data).unwrap();
        }
        let expected_high = index % 2 == 1;
        let executed = runtime.trigger(&mut frame, CONTROL).unwrap();
        native.use_block(CONTROL).unwrap();
        assert_eq!(
            executed,
            if expected_high {
                PrivateNotTrigger::Native
            } else {
                PrivateNotTrigger::Started
            },
            "only an actual unpowered request starts a frame at {index}"
        );
        if executed == PrivateNotTrigger::Started {
            let trigger_node = frame.positions[&CONTROL];
            let before = frame.node_state(trigger_node).powered;
            assert_eq!(runtime.trigger(&mut frame, CONTROL).unwrap(), PrivateNotTrigger::Busy);
            assert_eq!(frame.node_state(trigger_node).powered, before, "busy input remains with the caller");
        }
        let mut saw_falling = false;
        for _ in 0..6 {
            runtime.tick(&mut frame).unwrap();
            native.tick().unwrap();
            saw_falling |= !frame.node_state(lamp).powered;
            timing_mismatches += usize::from(frame.node_state(lamp).powered != native.node_state(lamp).powered);
        }
        assert_eq!(saw_falling, !expected_high, "observable Boolean pulse at frame {index}");
        assert_eq!(runtime.remaining, 0);
        frame.use_block(CONTROL).unwrap();
        native.use_block(CONTROL).unwrap();
    }
    for _ in 0..24 {
        runtime.tick(&mut frame).unwrap();
        native.tick().unwrap();
    }
    assert_eq!(
        frame.state.components[lamp.index()],
        native.state.components[lamp.index()],
        "native lamp at handoff"
    );
    assert_eq!(
        frame.state.components[runtime.plan.observer.index()],
        native.state.components[runtime.plan.observer.index()],
        "native observer at handoff"
    );
    assert_eq!(
        frame.future.scheduler.iter().collect::<Vec<_>>(),
        native.future.scheduler.iter().collect::<Vec<_>>(),
        "native scheduler at handoff"
    );
    assert!(frame.future.piston_requests.is_empty() && frame.future.deferred_pistons.is_empty());
    assert_eq!(runtime.executions, 2, "automatic source triggers execute the private frame");
    assert_eq!(frame.execution_counts().piston_actions, 0);
    assert!(native.execution_counts().piston_actions > 0);
    println!(
        "AUTOMATIC_PRIVATE_NOT frames=4 executions={} timing_mismatches={} callbacks={} native_callbacks={} native_actions={}",
        runtime.executions,
        timing_mismatches,
        frame.execution_counts().callbacks,
        native.execution_counts().callbacks,
        native.execution_counts().piston_actions
    );
}
