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
    for decline in &plan.declines {
        if let LayerWitness::Callback(occurrence) = decline.witness {
            let delivery = prepared.graph.routes[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
            let source = prepared.graph.sites[delivery.target.index()].source;
            eprintln!("unproved callback: {occurrence:?} {delivery:?}; target={source:?}");
            if let SiteSource::Contact(contact) = source {
                eprintln!("contact variants: {:?}", prepared.graph.contacts[contact.index()].variants);
            }
        }
    }
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
                    if observer == catalog.cells[0].uses[0].reset && original == prepared.graph.power_links[link as usize]
            ));
        }
    }

    // Ordinary Graph and native execution remain independent oracles; this evaluates
    // only the new Boolean artifact and does not execute a hidden wire/piston simulator.
    let (program, state) = prepared.graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
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
    let power = |link: PowerLink| {
        backend.guard_matches(link.guard, None) && backend.site_power(link.from, link.from_port).saturating_sub(link.attenuation) > 0
    };
    let mut values: Vec<bool> = Vec::with_capacity(region.expressions.len());
    for expression in &region.expressions {
        let value = match expression {
            LayerExpr::Constant(value) => *value,
            LayerExpr::Input(id) => match region.inputs[id.0 as usize].source {
                LayerInputSource::SourceAbove { site, port, threshold } => backend.site_power(site, port) > threshold,
                LayerInputSource::BoundaryEvent { occurrence, .. } => {
                    backend.program.nodes()[backend.program.routes()[occurrence.route.index()].source.index()].position == Some(CONTROL)
                }
                LayerInputSource::PowerContribution { link, .. } | LayerInputSource::LocalResetOutput { link, .. } => power(link),
                LayerInputSource::State(test) => state_predicate(backend, test),
            },
            LayerExpr::Not(operand) => !values[operand.0 as usize],
            LayerExpr::All(operands) => operands.iter().all(|operand| values[operand.0 as usize]),
            LayerExpr::Any(operands) => operands.iter().any(|operand| values[operand.0 as usize]),
        };
        values.push(value);
    }
    values
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

fn chain_world(dust: bool) -> PlotWorld {
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

#[test]
fn parser_composes_two_direct_cells_in_native_callback_order() {
    check_composed_chain(false);
}

#[test]
fn parser_contracts_a_private_two_wire_chain_into_an_expression_program() {
    check_composed_chain(true);
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
    let (program, state) = prepared.graph.clone().lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    native_step(
        &mut native,
        Step::SetLever {
            position: CONTROL,
            powered: false,
        },
    );
    backend.use_block(CONTROL).unwrap();
    let values = evaluate_entry(region, &backend);
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
    for token in &region.resource_outputs {
        if values[token.transfer_when.0 as usize] {
            assert_eq!(backend.state.resources[token.resource.index()].at, token.destination);
            assert!(!backend.state.resources[token.resource.index()].enabled);
        }
    }
}
