//! Real parser admission for a private stationary cell without any dust.
use super::*;
use crate::redpiler::analysis::instant::boundaries::infer_boundaries;
use crate::world::World;
use mchprs_blocks::blocks::{Lever, RedstoneObserver, RedstonePiston, RedstonePistonHead};
use mchprs_blocks::{BlockDirection, BlockFacing};

const BASE: BlockPos = BlockPos::new(8, 20, 8);
const CONTROL: BlockPos = BlockPos::new(7, 20, 8);
const PAYLOAD_OBSERVER: BlockPos = BlockPos::new(10, 21, 8);
const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(2, 17, 2), BlockPos::new(15, 24, 15));

#[path = "instant_layer_entry/interfaces.rs"]
mod interfaces;

#[path = "instant_layer_entry/wire_nodes.rs"]
mod wire_nodes;

fn wire_controlled_cell_world() -> PlotWorld {
    use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};
    let source = CONTROL.offset(BlockFace::West);
    let mut world = direct_cell_world();
    world.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(source, world.get_block(CONTROL));
    world.set_block(
        CONTROL,
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
fn turbo_requests_retain_the_actual_captured_update_route() {
    let source = CONTROL.offset(BlockFace::West);
    let mut world = wire_controlled_cell_world();
    let mut native = wire_controlled_cell_world();
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let (program, state) = prepared.graph.lower().unwrap();
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    let wire = backend.positions[&CONTROL];
    let route = backend.wire_plan(wire).turbo_route.unwrap();
    backend.use_block(source).unwrap();
    native_step(
        &mut native,
        Step::SetLever {
            position: source,
            powered: false,
        },
    );
    let request = backend.future.piston_requests.front().expect("the falling wire requested retraction");
    assert_eq!(request.source, Some(route));
    assert_eq!(backend.program.pistons()[request.piston.index()].position, BASE);
    assert_eq!(
        std::mem::size_of::<QueuedWireSite>(),
        8,
        "provenance must not enlarge the 64-bit wire queue entry"
    );
    let positions: Vec<_> = backend.program.sites().iter().map(|site| site.position).collect();
    for tick in 0..24 {
        backend.tick().unwrap();
        native.tick_interpreted();
        check_boundary(&backend, &native, &mut world, &positions, &format!("Turbo origin tick={tick}")).unwrap();
    }
}

fn direct_cell_world() -> PlotWorld {
    let mut world = empty_world();
    for (position, block) in [
        (
            BASE,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: true,
                    extended: true,
                },
            },
        ),
        (
            BASE.offset(BlockFace::East),
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: BlockFacing::East,
                    sticky: true,
                    short: false,
                },
            },
        ),
        (BASE.offset(BlockFace::East).offset(BlockFace::East), Block::RedstoneBlock),
        (
            BASE.offset(BlockFace::Top),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: false,
                },
            },
        ),
        (BASE.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {}),
        (CONTROL.offset(BlockFace::Bottom), Block::Stone {}),
        (
            CONTROL,
            Block::Lever {
                lever: Lever {
                    face: LeverFace::Floor,
                    facing: BlockDirection::North,
                    powered: true,
                },
            },
        ),
    ] {
        world.set_block(position, block);
    }
    world
}

#[test]
fn parser_admits_a_private_reset_cell_without_synthetic_contracts() {
    let world = direct_cell_world();
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let catalog = infer_boundaries(&prepared.graph, &monitor).unwrap();
    assert_eq!(catalog.cells.len(), 1, "normal mechanism admission must find this real cell");
    assert_eq!(catalog.cells[0].uses.len(), 1);
    assert_eq!(catalog.cells[0].resources.len(), 1);
    assert!(catalog.corridors.is_empty(), "the fixture has no dust to hide an arrival obligation");
    assert!(prepared.graph.wires.is_empty());
    prepared.graph.plans.instant.boundaries = Some(catalog);
    let plan = compile_layers(&prepared.graph, prepared.graph.plans.instant.boundaries.as_ref().unwrap(), &monitor).unwrap();
    assert_eq!(
        plan.regions.len(),
        1,
        "a real admitted cell must compile, not only synthetic equations: {:?}",
        plan.declines
    );
    assert!(plan.declines.is_empty());
    let region = &plan.regions[0];
    assert_eq!(region.layers.len(), 1);
    assert_eq!(region.layers[0].uses.len(), 1);
    let catalog = prepared.graph.plans.instant.boundaries.as_ref().unwrap();
    for (port_index, port) in catalog.ports.iter().enumerate() {
        if port.direction != BoundaryDirection::Output {
            continue;
        }
        let kept_as_output = region.outputs.iter().any(|output| output.port as usize == port_index);
        let kept_as_lifecycle = match port.kind {
            BoundaryPortKind::Callback { occurrence } => region.cold_lifecycle_occurrences.iter().any(|row| row.occurrence == occurrence),
            _ => false,
        };
        assert!(kept_as_output || kept_as_lifecycle, "a real perimeter obligation was erased: {port:?}");
        if let BoundaryPortKind::Power { link } = port.kind {
            let output = region.outputs.iter().find(|output| output.port as usize == port_index).unwrap();
            let LayerExpr::Input(input) = region.expressions[output.emitted_when.0 as usize] else {
                panic!("the preserved reset Power output must read its existing clock");
            };
            assert_eq!(region.inputs[input.0 as usize].role, LayerInputRole::AcceptanceRevalidation);
            assert!(matches!(
                region.inputs[input.0 as usize].source,
                LayerInputSource::LocalResetOutput { observer, link: original }
                    if Some(observer) == catalog.cells[0].uses[0].reset.observer() && original == prepared.graph.power_links[link as usize]
            ));
        }
    }

    // Ordinary Graph and native execution remain independent oracles; this evaluates
    // only the new Boolean artifact and does not execute a hidden wire/piston simulator.
    let (program, state) = prepared.graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    let mut native = direct_cell_world();
    let trigger = Step::SetLever {
        position: CONTROL,
        powered: false,
    };
    native_step(&mut native, trigger);
    backend.use_block(CONTROL).unwrap();
    let values = evaluate_entry(region, &backend);
    let use_ = &region.layers[0].uses[0];
    assert!(values[region.stationary_entry.0 as usize]);
    assert!(values[use_.requested.0 as usize]);
    assert!(values[use_.accepted.0 as usize]);
    assert_eq!(region.resource_outputs.len(), 1);
    let token = &region.resource_outputs[0];
    assert!(values[token.transfer_when.0 as usize]);
    assert!(values[token.source_absent_after.0 as usize]);

    let native_events = crate::redstone::piston::trace::capture(|| native.tick_interpreted());
    backend.tick().unwrap();
    assert_eq!(backend.execution_counts().piston_actions, 1);
    let accepted: Vec<_> = native_events
        .iter()
        .filter_map(|entry| match entry.operation {
            crate::redstone::piston::trace::Operation::Applied(event) => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].pos, BASE);
    assert_eq!(accepted[0].action, mchprs_world::PistonAction::Retract);
    let resource = backend.state.resources[token.resource.index()];
    assert_eq!(resource.at, token.destination);
    assert!(!resource.enabled);
    assert_eq!(native.get_block(BASE.offset(BlockFace::East).offset(BlockFace::East)), Block::Air);
    for output in &region.outputs {
        if let BoundaryPortKind::GuardState { test, .. } = catalog.ports[output.port as usize].kind {
            assert_eq!(
                values[output.emitted_when.0 as usize],
                state_predicate(&backend, test),
                "the layer's post-pull state output must match the actual action: {test:?}"
            );
        }
    }

    let positions: Vec<_> = prepared.graph.sites.iter().map(|site| site.position).collect();
    let mut projected = direct_cell_world();
    check_boundary(&backend, &native, &mut projected, &positions, "first pull").unwrap();
    for tick in 2..=16 {
        native.tick_interpreted();
        backend.tick().unwrap();
        check_boundary(&backend, &native, &mut projected, &positions, &format!("reset tick {tick}")).unwrap();
    }

    prepared.graph.plans.instant.layers = Some(plan);
    let export = crate::redpiler::analysis::export_json::from_graph(&prepared.report, &prepared.graph).unwrap();
    let directory = root().join("target/layered-instant-layers");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("direct-private-cell.graph.json"), export.to_bytes().unwrap()).unwrap();
}

fn evaluate_entry(region: &LayeredRegion, backend: &GraphBackend) -> Vec<bool> {
    let mut values: Vec<bool> = Vec::with_capacity(region.expressions.len());
    for expression in &region.expressions {
        let value = match expression {
            LayerExpr::Constant(value) => *value,
            LayerExpr::Input(id) => entry_input(region.inputs[id.0 as usize], backend),
            LayerExpr::Not(operand) => !values[operand.0 as usize],
            LayerExpr::All(operands) => operands.iter().all(|operand| values[operand.0 as usize]),
            LayerExpr::Any(operands) => operands.iter().any(|operand| values[operand.0 as usize]),
        };
        values.push(value);
    }
    values
}

fn entry_input(input: LayerInput, backend: &GraphBackend) -> bool {
    match input.source {
        LayerInputSource::WireDot { node } => {
            matches!(backend.state.components[node.index()], ComponentState::Wire(wire) if crate::redstone::wire::is_dot(wire))
        }
        LayerInputSource::SourceAbove { site, port, threshold } => backend.site_power(site, port) > threshold,
        LayerInputSource::BoundaryEvent { occurrence, .. } => {
            backend.program.nodes()[backend.program.routes()[occurrence.route.index()].source.index()].position == Some(CONTROL)
        }
        LayerInputSource::BoundaryRequest { occurrence, actor, .. } => backend.future.piston_requests.iter().any(|request| {
            request.piston == actor && request.action == mchprs_world::PistonAction::Retract && request.source == Some(occurrence.route)
        }),
        LayerInputSource::PowerContribution { link, .. } | LayerInputSource::LocalResetOutput { link, .. } => {
            backend.guard_matches(link.guard, None) && backend.site_power(link.from, link.from_port).saturating_sub(link.attenuation) > 0
        }
        LayerInputSource::State(test) | LayerInputSource::BoundaryState { test, .. } => state_predicate(backend, test),
        LayerInputSource::InstantFired { .. } => panic!("legacy instant-entry fixture has no logical firing latch"),
    }
}

fn state_predicate(backend: &GraphBackend, test: StateTest) -> bool {
    match test {
        StateTest::PistonExtended(actor) => backend.state.pistons[actor.index()].extended,
        StateTest::PistonHeadPresent(actor) => backend.state.pistons[actor.index()].head_present,
        StateTest::PistonAvailability { piston, is } => backend.state.pistons[piston.index()].availability == is,
        StateTest::ResourceAt { resource, contact } => backend.state.resources[resource.index()].at == contact,
        StateTest::ResourceEnabled(resource) => backend.state.resources[resource.index()].enabled,
    }
}

#[test]
fn unproved_payload_observation_is_not_silently_dropped() {
    let mut world = direct_cell_world();
    world.set_block(
        PAYLOAD_OBSERVER,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let catalog = infer_boundaries(&prepared.graph, &monitor).unwrap();
    assert_eq!(catalog.cells.len(), 1, "the stationary cell is still admitted before layer analysis");
    assert!(catalog.ports.iter().any(|port| {
        matches!(port.kind, BoundaryPortKind::ObserverWatch { observer, .. }
            if prepared.graph.nodes[observer.index()].position == Some(PAYLOAD_OBSERVER))
    }));
    let source = prepared.graph.clone();
    let plan = compile_layers(&prepared.graph, &catalog, &monitor).unwrap();
    assert!(
        plan.regions.is_empty(),
        "a missing geometry output proof must not yield a compiled region"
    );
    assert_eq!(plan.declines.len(), 1);
    assert_eq!(plan.declines[0].reason, LayerDeclineReason::BoundaryEffectUnsupported);
    assert_eq!(prepared.graph, source, "the ordinary executable graph remains intact");
}

fn direct_chain_world() -> PlotWorld {
    chain_world(false)
}

#[test]
fn downstream_reset_is_derived_from_restored_input_and_requires_its_update() {
    use crate::redpiler::analysis::instant::cells::discover_cells;
    for dust in [false, true] {
        let second = BlockPos::new(if dust { 13 } else { 11 }, 20, 8);
        let mut world = chain_world(dust);
        world.set_block(second.offset(BlockFace::Top), Block::Air);
        let monitor = TaskMonitor::default();
        let prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
        let source = &prepared.graph;
        let actor = PistonId::new(source.pistons.iter().position(|actor| actor.position == second).unwrap() as u32);
        let cells = discover_cells(source, &monitor).unwrap();
        let use_ = cells
            .cells
            .iter()
            .flat_map(|cell| cell.uses.iter())
            .find(|use_| use_.actor == actor)
            .unwrap_or_else(|| panic!("downstream cell inherits upstream reset (dust={dust}); declines={:?}", cells.declined));
        let CellReset::InputRestoration { notification, sample, .. } = use_.reset else {
            panic!("downstream actor has no observer or own-payload return")
        };
        assert_eq!(
            notification == sample,
            !dust,
            "a wire reset retains its separate notification and receiver witnesses"
        );
        let mut cut = source.clone();
        for route in &mut cut.routes {
            route.deliveries = route
                .deliveries
                .iter()
                .copied()
                .filter(|delivery| {
                    !matches!(delivery.receiver,Some(CallbackReceiver::PistonBase(target) | CallbackReceiver::PistonHead(target)) if target == actor)
                        && delivery.receiver != Some(CallbackReceiver::Component(source.pistons[actor.index()].node))
                })
                .collect();
        }
        let cut_cells = discover_cells(&cut, &monitor).unwrap();
        assert!(
            cut_cells.cells.iter().all(|cell| cell.uses.iter().all(|use_| use_.actor != actor)),
            "power without sampling cannot certify a reset"
        );

        let (program, state) = source.clone().lower().unwrap();
        let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
        let mut native = chain_world(dust);
        native.set_block(second.offset(BlockFace::Top), Block::Air);
        let mut projected = world;
        let positions: Vec<_> = source.sites.iter().map(|site| site.position).collect();
        native_step(
            &mut native,
            Step::SetLever {
                position: CONTROL,
                powered: false,
            },
        );
        backend.use_block(CONTROL).unwrap();
        for tick in 0..16 {
            native.tick_interpreted();
            backend.tick().unwrap();
            check_boundary(
                &backend,
                &native,
                &mut projected,
                &positions,
                &format!("inherited reset dust={dust} tick={tick}"),
            )
            .unwrap();
        }
    }
}

pub(super) fn chain_world(dust: bool) -> PlotWorld {
    let second = BlockPos::new(if dust { 13 } else { 11 }, 20, 8);
    let mut world = direct_cell_world();
    for (position, block) in [
        (
            second,
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::South,
                    sticky: true,
                    extended: true,
                },
            },
        ),
        (
            second.offset(BlockFace::South),
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: BlockFacing::South,
                    sticky: true,
                    short: false,
                },
            },
        ),
        (second.offset(BlockFace::South).offset(BlockFace::South), Block::RedstoneBlock),
        (
            second.offset(BlockFace::Top),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: false,
                },
            },
        ),
        (second.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {}),
    ] {
        world.set_block(position, block);
    }
    if dust {
        use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};
        for x in [11, 12] {
            let position = BlockPos::new(x, 20, 8);
            world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                position,
                Block::RedstoneWire {
                    wire: RedstoneWire::new(
                        RedstoneWireSide::Side,
                        RedstoneWireSide::Side,
                        RedstoneWireSide::Side,
                        RedstoneWireSide::Side,
                        26 - x as u8,
                    ),
                },
            );
        }
    }
    world
}

fn wire_arrival_world(observer: bool) -> PlotWorld {
    let mut world = chain_world(true);
    if observer {
        world.set_block(
            BlockPos::new(11, 21, 8),
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::Down,
                    powered: false,
                },
            },
        );
    } else {
        let position = BlockPos::new(11, 20, 8);
        let Block::RedstoneWire { mut wire } = world.get_block(position) else {
            unreachable!()
        };
        wire.power = 0;
        world.set_block(position, Block::RedstoneWire { wire });
    }
    world
}

#[test]
fn parser_composes_two_direct_cells_in_native_callback_order() {
    check_composed_chain(false);
}

#[test]
fn parser_contracts_a_private_two_wire_chain_into_an_expression_program() {
    check_composed_chain(true);
}

#[test]
fn four_cell_wave_executes_without_internal_turbo_and_preserves_reset() {
    use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};
    let build = || {
        let mut world = chain_world(true);
        for (position, facing) in [
            (BlockPos::new(13, 20, 13), BlockFacing::East),
            (BlockPos::new(18, 20, 13), BlockFacing::South),
        ] {
            world.set_block(
                position,
                Block::Piston {
                    piston: RedstonePiston {
                        facing,
                        sticky: true,
                        extended: true,
                    },
                },
            );
            let face: BlockFace = facing.into();
            world.set_block(
                position.offset(face),
                Block::PistonHead {
                    head: RedstonePistonHead {
                        facing,
                        sticky: true,
                        short: false,
                    },
                },
            );
            world.set_block(position.offset(face).offset(face), Block::RedstoneBlock);
            world.set_block(
                position.offset(BlockFace::Top),
                Block::Observer {
                    observer: RedstoneObserver {
                        facing: BlockFacing::Down,
                        powered: false,
                    },
                },
            );
            world.set_block(position.offset(BlockFace::Top).offset(BlockFace::Top), Block::Stone {});
        }
        for (position, power) in [
            (BlockPos::new(13, 20, 11), 15),
            (BlockPos::new(13, 20, 12), 14),
            (BlockPos::new(16, 20, 13), 15),
            (BlockPos::new(17, 20, 13), 14),
        ] {
            world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
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
        world
    };
    let world = build();
    let mut native = build();
    let monitor = TaskMonitor::default();
    let bounds = (BOUNDS.0, BlockPos::new(24, 24, 24));
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let catalog = infer_boundaries(&prepared.graph, &monitor).unwrap();
    let plan = compile_layers(&prepared.graph, &catalog, &monitor).unwrap();
    assert_eq!(plan.regions.len(), 1, "{:?}", plan.declines);
    prepared.graph.plans.instant.boundaries = Some(catalog);
    prepared.graph.plans.instant.layers = Some(plan);
    let positions: Vec<_> = prepared.graph.sites.iter().map(|site| site.position).collect();
    let (program, state) = prepared.graph.lower().unwrap();
    assert_eq!(program.instant_kernels().len(), 1);
    assert_eq!(program.instant_kernels()[0].actors.len(), 4);
    assert_eq!(program.instant_kernels()[0].wires.len(), 6);
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    backend.use_block(CONTROL).unwrap();
    native_step(
        &mut native,
        Step::SetLever {
            position: CONTROL,
            powered: false,
        },
    );
    let mut projected = world;
    for tick in 0..32 {
        backend.tick().unwrap();
        native.tick_interpreted();
        check_boundary(&backend, &native, &mut projected, &positions, &format!("four-cell tick {tick}")).unwrap();
        if tick == 0 {
            assert_eq!(backend.execution_counts().instant_kernel_invocations, 4);
            assert_eq!(backend.execution_counts().piston_actions, 4);
            assert_eq!(backend.execution_counts().wire_walks, 0);
        }
    }
}

#[test]
fn public_o4_pipeline_matches_native_with_trusted_and_unrelated_request_origins() {
    for trusted_origin in [false, true] {
        let options = crate::redpiler::CompilerOptions {
            optimization_level: Some(4),
            stable_instant_inputs: true,
            ..Default::default()
        };
        let world = chain_world(true);
        let mut native = chain_world(true);
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
        passes::run_graph_pipeline(
            &mut prepared.graph,
            &options,
            &prepared.entry,
            &passes::default_graph_pipeline(&options),
            &monitor,
        )
        .unwrap();
        assert!(prepared.graph.plans.instant.layers.is_none());
        assert_eq!(prepared.graph.plans.instant_events.len(), 1);
        let (program, state) = prepared.graph.lower().unwrap();
        let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
        assert_eq!(backend.program.instant_kernels().len(), 1);
        let positions: Vec<_> = backend.program.sites().iter().map(|site| site.position).collect();
        backend.use_block(CONTROL).unwrap();
        native_step(
            &mut native,
            Step::SetLever {
                position: CONTROL,
                powered: false,
            },
        );
        if !trusted_origin {
            backend.future.piston_requests.set_front_source(None);
        }
        let mut projected = world;
        for tick in 0..24 {
            backend.tick().unwrap();
            native.tick_interpreted();
            check_boundary(
                &backend,
                &native,
                &mut projected,
                &positions,
                &format!("public wave origin={trusted_origin} tick={tick}"),
            )
            .unwrap();
            if tick == 0 {
                assert!(backend.execution_counts().instant_kernel_invocations >= 2);
                assert!(backend.execution_counts().wire_walks > 0);
            }
        }
    }
}

#[test]
fn hot_entry_arrival_or_native_fallback_preserves_observer_work() {
    for observer in [false, true] {
        let world = wire_arrival_world(observer);
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
        let catalog = infer_boundaries(&prepared.graph, &monitor).unwrap();
        let selected_plan = compile_layers(&prepared.graph, &catalog, &monitor).unwrap();
        if observer {
            assert!(selected_plan.regions.is_empty(), "the effectful observer keeps this component on Graph");
        } else {
            assert!(!selected_plan.regions.is_empty(), "the certified hot-entry cable compiles");
        }
        let label = if observer { "native observer fallback" } else { "hot-entry arrival" };
        prepared.graph.plans.instant.boundaries = Some(catalog);
        prepared.graph.plans.instant.layers = Some(selected_plan);
        prepared.graph.validate().unwrap();
        let (program, state) = prepared.graph.clone().lower().unwrap();
        let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
        let mut native = wire_arrival_world(observer);
        let positions: Vec<_> = prepared.graph.sites.iter().map(|site| site.position).collect();
        let mut projected = world;
        for (phase, powered) in [("cold retract", false), ("warm extend", true), ("hot retract", false)] {
            backend.use_block(CONTROL).unwrap();
            native_step(&mut native, Step::SetLever { position: CONTROL, powered });
            for tick in 0..24 {
                backend.tick().unwrap();
                native.tick_interpreted();
                check_boundary(&backend, &native, &mut projected, &positions, &format!("{label} {phase} tick {tick}")).unwrap();
            }
        }
        if !observer {
            assert!(backend.execution_counts().instant_kernel_invocations > 0, "the valid hot cycle executes its PullWave");
        }
        let mut handed_back = wire_arrival_world(observer);
        backend.reset(&mut handed_back);
        assert_eq!(
            first_difference(&future(&native, &positions), &future(&handed_back, &positions)),
            None,
            "reset must hand pending work back without changing its FIFO-visible state"
        );
        for tick in 0..8 {
            native.tick_interpreted();
            handed_back.tick_interpreted();
            assert_eq!(
                first_difference(&future(&native, &positions), &future(&handed_back, &positions)),
                None,
                "native continuation after reset tick {tick}"
            );
        }
    }

}

fn check_composed_chain(dust: bool) {
    let second = BlockPos::new(if dust { 13 } else { 11 }, 20, 8);
    let world = if dust { chain_world(true) } else { direct_chain_world() };
    let mut native = if dust { chain_world(true) } else { direct_chain_world() };
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&world, BOUNDS, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let catalog = infer_boundaries(&prepared.graph, &monitor).unwrap();
    assert_eq!(catalog.cells.len(), 2);
    assert_eq!(catalog.corridors.len(), usize::from(dust));
    let plan = compile_layers(&prepared.graph, &catalog, &monitor).unwrap();
    assert_eq!(plan.regions.len(), 1, "the real two-cell component must compile: {:?}", plan.declines);
    assert!(plan.declines.is_empty());
    let region = &plan.regions[0];
    prepared.graph.plans.instant.boundaries = Some(catalog);
    prepared.graph.plans.instant.layers = Some(plan.clone());
    prepared.graph.validate().unwrap();
    let mut stale = prepared.graph.clone();
    stale.plans.instant.layers.as_mut().unwrap().programs = Box::new([]);
    assert!(stale.validate().is_err(), "compact programs cannot be detached from their layer evidence");
    let (program, state) = prepared.graph.clone().lower().unwrap();
    assert_eq!(program.instant_kernels().len(), 1, "the certified region must lower to an executable");
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    native_step(
        &mut native,
        Step::SetLever {
            position: CONTROL,
            powered: false,
        },
    );
    backend.use_block(CONTROL).unwrap();
    let values = evaluate_entry(region, &backend);
    let kernel = &plan.programs[0];
    assert_eq!(
        *kernel,
        crate::redpiler::analysis::instant::layers::lower_region(region, &monitor).unwrap()
    );
    let mut scratch = vec![false; kernel.expressions.len()];
    kernel.evaluate(&mut scratch, |input| entry_input(input, &backend)).unwrap();
    for &(kind, root) in &kernel.roots {
        let original = match kind {
            LayerRoot::StationaryEntry => region.stationary_entry,
            LayerRoot::Output(index) => region.outputs[index].emitted_when,
            LayerRoot::Transfer(index) => region.resource_outputs[index].transfer_when,
            LayerRoot::SourceAbsent(index) => region.resource_outputs[index].source_absent_after,
            LayerRoot::Accepted { layer, occurrence } => region.layers[layer].uses[occurrence].accepted,
            LayerRoot::Requested { layer, occurrence } => region.layers[layer].uses[occurrence].requested,
        };
        assert_eq!(scratch[root.0 as usize], values[original.0 as usize]);
    }
    let owned_wires: std::collections::BTreeSet<_> = region
        .corridors
        .iter()
        .flat_map(|&id| {
            prepared.graph.plans.instant.boundaries.as_ref().unwrap().corridors[id as usize]
                .wires
                .iter()
                .copied()
        })
        .collect();
    for input in &kernel.inputs {
        if let LayerInputSource::SourceAbove { site, .. } = input.source {
            if let SiteSource::Component(node) = prepared.graph.sites[site.index()].source {
                assert!(!owned_wires.contains(&node), "the executable must not read eliminated wire state");
            }
        }
    }
    println!(
        "chain dust={dust}: actors=2, eliminated wire operands={}, expressions={} -> {}, inputs={} -> {}, roots={}",
        owned_wires.len(),
        region.expressions.len(),
        kernel.expressions.len(),
        region.inputs.len(),
        kernel.inputs.len(),
        kernel.roots.len()
    );
    let predicted: Vec<_> = region
        .layers
        .iter()
        .flat_map(|layer| &layer.uses)
        .filter(|use_| values[use_.accepted.0 as usize])
        .map(|use_| prepared.graph.pistons[use_.actor.index()].position)
        .collect();
    assert_eq!(predicted, vec![BASE, second]);
    let events = crate::redstone::piston::trace::capture(|| native.tick_interpreted());
    let actual: Vec<_> = events
        .iter()
        .filter_map(|entry| match entry.operation {
            crate::redstone::piston::trace::Operation::Applied(event) if event.action == mchprs_world::PistonAction::Retract => Some(event.pos),
            _ => None,
        })
        .collect();
    assert_eq!(predicted, actual);
    backend.tick().unwrap();
    assert_eq!(
        backend.execution_counts().instant_kernel_invocations,
        predicted.len() as u64,
        "the reduced program must replace execution, not only predict it"
    );
    assert_eq!(backend.execution_counts().wire_walks, 0, "the covered computation must not run Turbo");
    for token in &region.resource_outputs {
        if values[token.transfer_when.0 as usize] {
            assert_eq!(backend.state.resources[token.resource.index()].at, token.destination);
            assert!(!backend.state.resources[token.resource.index()].enabled);
        }
    }
    let positions: Vec<_> = prepared.graph.sites.iter().map(|site| site.position).collect();
    let mut projected = chain_world(dust);
    check_boundary(&backend, &native, &mut projected, &positions, "composed first pull").unwrap();
    for tick in 2..=16 {
        native.tick_interpreted();
        backend.tick().unwrap();
        check_boundary(&backend, &native, &mut projected, &positions, &format!("composed reset tick {tick}")).unwrap();
    }
    let export = crate::redpiler::analysis::export_json::from_graph(&prepared.report, &prepared.graph).unwrap();
    let directory = root().join("target/layered-instant-layers");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(if dust {
            "private-wire-chain.graph.json"
        } else {
            "private-direct-chain.graph.json"
        }),
        export.to_bytes().unwrap(),
    )
    .unwrap();
}
