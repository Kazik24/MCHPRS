//! Exact admission masks preserve captured dispatch and dynamic guard fallbacks.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::passes::{PassContext, planning::PlanBuilder};
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Lever, LeverFace, RedstonePiston};

fn draft(near_contact: bool) -> CompileGraph {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    world.set_block(
        BlockPos::new(5, 20, 5),
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
        },
    );
    for x in [6, 7] {
        let pos = BlockPos::new(x, 20, 5);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(pos, Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(BlockPos::new(8, 20, 5), Block::RedstoneLamp { lit: false });
    world.set_block(
        BlockPos::new(12, 20, 12),
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFace::East.into(),
                sticky: false,
                extended: false,
            },
        },
    );
    if near_contact {
        world.set_block(BlockPos::new(12, 19, 11), Block::Obsidian {});
        world.set_block(BlockPos::new(12, 20, 11), Block::RedstoneWire { wire: Default::default() });
    }
    passes::rich::prepare(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
        vec![],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
    .graph
}

fn input_callback_ports_world() -> PlotWorld {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for z in [5, 10] {
        world.set_block(BlockPos::new(5, 19, z), Block::Stone {});
        world.set_block(
            BlockPos::new(5, 20, z),
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
            },
        );
        for x in [6, 7] {
            let position = BlockPos::new(x, 20, z);
            world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(position, Block::RedstoneWire { wire: Default::default() });
        }
        world.set_block(BlockPos::new(8, 20, z), Block::RedstoneLamp { lit: false });
    }
    world.set_block(BlockPos::new(12, 19, 12), Block::Stone {});
    world.set_block(
        BlockPos::new(12, 20, 12),
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFace::East.into(),
                sticky: false,
                extended: false,
            },
        },
    );
    world
}

#[test]
fn logical_input_callback_ports_are_exact_and_preserve_native_lamp_callbacks() {
    let mut world = input_callback_ports_world();
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
    let mut backend = GraphBackend::compile(&world, bounds, vec![], Arc::default()).unwrap();
    let source = backend.positions[&BlockPos::new(5, 20, 5)];
    let expected_target = backend.positions[&BlockPos::new(6, 20, 5)];
    let clock_piston = backend
        .program
        .pistons()
        .iter()
        .enumerate()
        .find_map(|(index, piston)| {
            (backend.program.nodes()[piston.node.index()].position == Some(BlockPos::new(12, 20, 12)))
                .then_some(crate::redpiler::program_graph::PistonId::new(index as u32))
        })
        .expect("the fixture's isolated piston supplies a clock-state guard");
    let clock_node = backend.program.pistons()[clock_piston.index()].node;
    let clock_guard = backend
        .program
        .guards()
        .iter()
        .position(|guard| {
            *guard == crate::redpiler::program_graph::Guard::State(crate::redpiler::program_graph::StateTest::PistonExtended(clock_piston))
        })
        .map(|index| crate::redpiler::program_graph::GuardId::new(index as u32))
        .expect("piston-state guard reads the logical clock state");
    backend.set_node(source, true, 15, None, false).unwrap();
    let (route, occurrence, delivery) = backend
        .program
        .routes()
        .iter()
        .enumerate()
        .filter(|(_, route)| route.source == source)
        .find_map(|(route_index, route)| {
            route.deliveries.iter().enumerate().find_map(|(ordinal, delivery)| {
                (delivery.receiver == Some(CallbackReceiver::Component(expected_target))
                    && matches!(delivery.kind, DeliveryKind::Update { .. })
                    && delivery.view == OperandView::Live
                    && backend.guard_matches(delivery.guard, None))
                .then_some((
                    RouteId::new(route_index as u32),
                    RouteOccurrence {
                        route: RouteId::new(route_index as u32),
                        ordinal: ordinal as u32,
                    },
                    *delivery,
                ))
            })
        })
        .expect("native lever route updates the first dust node");

    backend.logical_device_owner = vec![None; backend.program.nodes().len()];
    backend.logical_device_owner[expected_target.index()] = Some(0);
    backend.logical_device_owner[clock_node.index()] = Some(0);
    backend.logical_input_callbacks.insert(occurrence);

    let mut clock_guarded_delivery = delivery;
    clock_guarded_delivery.guard = clock_guard;
    for extended in [false, true] {
        backend.state.pistons[clock_piston.index()].extended = extended;
        assert_eq!(
            backend.guard_matches(clock_guard, None),
            extended,
            "the guard observes clock state {extended}"
        );
        let callbacks_before = backend.counts.callbacks;
        backend.deliver_callback(route, clock_guarded_delivery, None, Some(occurrence)).unwrap();
        assert_eq!(backend.counts.callbacks, callbacks_before, "the certified input callback is skipped");
        assert_eq!(
            backend.state.pistons[clock_piston.index()].extended,
            extended,
            "port consumption never mutates the clock"
        );
        assert_eq!(
            backend.wire_state(expected_target).power,
            0,
            "the owned wire is sampled by its device, not mutated by Graph"
        );
    }

    let wrong_ordinal = RouteOccurrence {
        route,
        ordinal: occurrence.ordinal + 1,
    };
    backend.logical_input_callbacks.clear();
    backend.logical_input_callbacks.insert(wrong_ordinal);
    backend.state.pistons[clock_piston.index()].extended = true;
    assert!(matches!(
        backend.deliver_callback(route, clock_guarded_delivery, None, Some(occurrence)),
        Err(GraphExecutionError::UnsupportedComponent {
            kind: "ordinary callback crosses a logical-device scope",
            ..
        })
    ));

    let callbacks_before = backend.counts.callbacks;
    backend.use_block(BlockPos::new(5, 20, 10)).unwrap();
    backend.flush(&mut world, false);
    assert!(backend.counts.callbacks > callbacks_before, "ordinary native callbacks still run");
    assert!(matches!(world.get_block(BlockPos::new(8, 20, 10)), Block::RedstoneLamp { lit: true }));

    backend.reset(&mut world);
    assert!(backend.logical_input_callbacks.is_empty(), "reset clears persistent input callback ports");
}

#[test]
fn marked_kernel_route_uses_general_delivery_with_an_active_kernel() {
    // A certified route marker must preserve ordinary callback behavior.
    let world = super::adder_tests::composed_chain_world(true);
    let bounds = (BlockPos::new(2, 17, 2), BlockPos::new(15, 24, 15));
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let catalog = crate::redpiler::analysis::instant::boundaries::infer_boundaries(&prepared.graph, &monitor).unwrap();
    let plan = crate::redpiler::analysis::instant::layers::compile_layers(&prepared.graph, &catalog, &monitor).unwrap();
    assert!(!plan.regions.is_empty(), "fixture must produce an installed instant kernel");
    prepared.graph.plans.instant.boundaries = Some(catalog);
    prepared.graph.plans.instant.layers = Some(plan);
    let (program, state) = prepared.graph.lower().unwrap();
    let mut baseline = GraphBackend::new_unactivated(program.clone(), state.clone(), 0).unwrap();
    let mut candidate = GraphBackend::new_unactivated(program.clone(), state, 0).unwrap();
    let (kernel, route, kernel_deliveries) = program
        .instant_kernels()
        .iter()
        .find_map(|kernel| {
            kernel.routes.iter().find_map(|row| {
                let original_count = program.routes()[row.route.index()].deliveries.len();
                let delivered_count = row
                    .steps
                    .iter()
                    .filter(|step| matches!(&step.kind, crate::redpiler::program_graph::KernelRouteStepKind::Deliver(_)))
                    .count();
                (delivered_count > 0 && delivered_count < original_count).then_some((kernel.clone(), row.route, delivered_count))
            })
        })
        .expect("fixture contains an optimized route with a reduced callback tape");
    let original_deliveries = program.routes()[route.index()].deliveries.len();
    assert!(
        kernel_deliveries < original_deliveries,
        "the optimized row omits callbacks from the original route"
    );
    candidate.instant_kernel.active = Some(kernel);
    candidate.general_delivery_routes[route.index()] = true;
    let baseline_capture = (0..program.sites().len())
        .map(|index| baseline.observe_site(SiteId::new(index as u32)))
        .collect::<Vec<_>>();
    let candidate_capture = (0..program.sites().len())
        .map(|index| candidate.observe_site(SiteId::new(index as u32)))
        .collect::<Vec<_>>();
    baseline.deliver_route(route, Some(&baseline_capture)).unwrap();
    candidate.deliver_route(route, Some(&candidate_capture)).unwrap();
    assert_eq!(candidate.state, baseline.state, "marked route executes its original callbacks");
    assert_eq!(
        candidate.future.piston_requests, baseline.future.piston_requests,
        "marked route preserves native queued piston work"
    );
    assert_eq!(candidate.counts.callbacks, baseline.counts.callbacks);
}

fn compile(graph: &mut CompileGraph) {
    let monitor = TaskMonitor::default();
    let options = Default::default();
    let entry = EntryState {
        logical_tick: 0,
        pending_ticks: vec![],
    };
    let context = PassContext::new(&monitor, &options, &entry);
    passes::index_turbo_callbacks::IndexTurboCallbacks.run(graph, &context).unwrap();
    passes::compile_wire_callbacks::CompileWireCallbacks.run(graph, &context).unwrap();
}
fn backend(graph: CompileGraph) -> GraphBackend {
    let (program, state) = graph.lower().unwrap();
    GraphBackend::new_unactivated(program, state, 0).unwrap()
}
fn assert_selections(backend: &GraphBackend) {
    for (index, wire) in backend.program.wires().iter().enumerate() {
        let route = wire.turbo_route.unwrap();
        for (slot, &site) in wire.canonical_neighbors.iter().enumerate() {
            let observed = backend.observe_site(site).unwrap();
            assert_eq!(
                backend.wire_route_admitted(index, route, slot, site, Some(&observed)),
                backend.wire_route_delivery(index, route, slot, site, &observed).is_some(),
                "wire {index} slot {slot}"
            );
        }
    }
}
#[test]
fn wire_callbacks_fixed_masks_ignore_mutable_receiver_power_and_reject_forgery() {
    let mut graph = draft(false);
    compile(&mut graph);
    assert!(
        graph
            .wires
            .iter()
            .all(|wire| wire.execution.callback_selection.unwrap().known == WireCallbackSelection::ALL)
    );
    let mut forged = graph.clone();
    forged.wires[0].execution.callback_selection.as_mut().unwrap().admitted ^= 1;
    assert!(matches!(forged.validate(), Err(GraphBuildError::InvalidWireCallbackSelection { .. })));
    let mut backend = backend(graph);
    assert!(
        backend.program.turbo_callback_indices().is_empty(),
        "fully decided programs need no runtime search index"
    );
    for strength in [0, 1, 15] {
        for state in &mut backend.state.components {
            if let ComponentState::Electrical(state) = state {
                state.output_strength = strength;
                state.powered = strength > 0;
                state.repeater_locked = strength == 1;
            }
        }
        assert_selections(&backend);
    }
    let general = backend.program.without_fixed_wire_callbacks();
    assert_eq!(
        general.turbo_callback_indices().len(),
        general.wires().len(),
        "control restores original grouped search"
    );
}
fn replace_guard(graph: &mut CompileGraph, guard: Guard) -> (usize, usize) {
    let index = graph
        .wires
        .iter()
        .position(|wire| graph.nodes[wire.node.index()].position == Some(BlockPos::new(6, 20, 5)))
        .unwrap();
    let route = graph.wires[index].turbo_route.unwrap();
    let target = graph
        .sites
        .iter()
        .position(|site| site.position == BlockPos::new(7, 20, 5))
        .map(|index| SiteId::new(index as u32))
        .unwrap();
    let slot = graph.wires[index].canonical_neighbors.iter().position(|site| *site == target).unwrap();
    let id = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(guard);
    let rows = graph.routes[route.index()]
        .deliveries
        .iter_mut()
        .filter(|row| row.target == target && row.view == OperandView::Captured && row.receiver.is_some())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    let row = rows.into_iter().next().unwrap();
    row.guard = id;
    let sequence = row.sequence;
    let kind = row.link_kind();
    for link in &mut graph.links {
        if link.callback.route == route && link.callback.sequence == sequence {
            link.kind = kind;
        }
    }
    (index, slot)
}
#[test]
fn wire_callbacks_dynamic_state_keeps_exact_fallback_and_capture_identity() {
    let mut graph = draft(false);
    let (index, slot) = replace_guard(&mut graph, Guard::State(StateTest::PistonExtended(PistonId::new(0))));
    compile(&mut graph);
    assert_eq!(graph.wires[index].execution.callback_selection.unwrap().known & (1 << slot), 0);
    let mut engine = backend(graph);
    for extended in [false, true] {
        engine.state.pistons[0].extended = extended;
        assert_selections(&engine);
    }
    let mut graph = draft(false);
    let other = graph.wires[0].canonical_neighbors[3];
    let (index, slot) = replace_guard(
        &mut graph,
        Guard::Site {
            site: other,
            view: OperandView::Captured,
            test: SiteTest::IsWire,
        },
    );
    compile(&mut graph);
    let plan = graph.wires[index].execution.callback_selection.unwrap();
    assert_ne!(plan.known & (1 << slot), 0);
    assert_eq!(plan.admitted & (1 << slot), 0);
    assert_selections(&backend(graph));
}
#[test]
fn wire_callbacks_contacts_keep_general_selection() {
    let mut graph = draft(true);
    compile(&mut graph);
    let index = graph
        .wires
        .iter()
        .position(|wire| graph.nodes[wire.node.index()].position == Some(BlockPos::new(12, 20, 11)))
        .unwrap();
    assert_ne!(graph.wires[index].execution.callback_selection.unwrap().known, WireCallbackSelection::ALL);
    assert_selections(&backend(graph));
}
