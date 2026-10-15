//! Complete singleton Turbo replay against the general walk and interpreter.
use super::fuzz_tests::{graph_operations_since, native_operations};
use super::*;
use crate::plot::PlotWorld;
use crate::redstone;
use crate::redstone::instant_piston_tests::capture_at;
use crate::world::storage::Chunk;
use mchprs_blocks::BlockDirection;
use mchprs_blocks::blocks::{Lever, LeverFace, RedstoneComparator};

const ROOT: BlockPos = BlockPos::new(8, 20, 8);
const SOURCE: BlockPos = BlockPos::new(7, 20, 8);
const LAMP: BlockPos = BlockPos::new(9, 20, 8);
const ABOVE: BlockPos = BlockPos::new(8, 21, 8);
const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));

fn world(source: Block) -> PlotWorld {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    world.set_block(ROOT, Block::RedstoneWire { wire: Default::default() });
    world.set_block(SOURCE, source);
    if matches!(source, Block::RedstoneComparator { .. }) {
        world.set_block_entity(SOURCE, BlockEntity::Comparator { output_strength: 0 });
    }
    for position in [ROOT, SOURCE] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    for position in [LAMP, ABOVE] {
        world.set_block(position, Block::RedstoneLamp { lit: false });
    }
    world
}

fn draft(world: &PlotWorld) -> CompileGraph {
    passes::rich::prepare(world, BOUNDS, vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph
}

fn backend(mut graph: CompileGraph, isolated: bool) -> GraphBackend {
    if isolated {
        for index in 0..graph.wires.len() {
            let facts = WireQueryFacts::infer(&graph, &graph.wires[index]);
            assert!(facts.isolated);
            graph.wires[index].execution.query_facts = Some(facts);
        }
    }
    let (program, state) = graph.lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    backend.set_test_recording(true);
    backend
}

fn assert_same_execution(general: &GraphBackend, isolated: &GraphBackend) {
    assert_eq!(isolated.state, general.state);
    assert_eq!(isolated.execution_counts(), general.execution_counts());
    assert_eq!(isolated.callback_order, general.callback_order);
    assert_eq!(isolated.observed_operations, general.observed_operations);
    assert_eq!(isolated.scheduled_ticks(), general.scheduled_ticks());
    assert_eq!(isolated.future.pending, general.future.pending);
    let sounds = |backend: &GraphBackend| {
        backend
            .effects
            .iter()
            .map(|effect| match effect {
                PresentationEffect::Sound {
                    position,
                    id,
                    category,
                    volume,
                    pitch,
                } => (*position, *id, *category, *volume, *pitch),
                PresentationEffect::BlockAction { .. } => panic!("these electrical fixtures emit only sounds"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(sounds(isolated), sounds(general));
    let deliveries = |backend: &GraphBackend| {
        backend
            .trace()
            .iter()
            .filter(|line| line.starts_with("wire deliver "))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(deliveries(isolated), deliveries(general));
}

fn replay_count(backend: &GraphBackend) -> usize {
    backend.trace().iter().filter(|line| line.starts_with("wire isolated replay ")).count()
}

#[test]
fn singleton_replay_preserves_all_strength_changes_order_duplicates_and_no_change() {
    let world = world(Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
    });
    let graph = draft(&world);
    let mut general = backend(graph.clone(), false);
    let mut isolated = backend(graph, true);
    let root = general.positions[&ROOT];
    let source = general.positions[&SOURCE];
    for old in 0..=15 {
        for new in 0..=15 {
            for backend in [&mut general, &mut isolated] {
                backend.test_recorder.clear();
                let ComponentState::Wire(wire) = &mut backend.state.components[root.index()] else {
                    unreachable!()
                };
                wire.power = old;
                let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
                    unreachable!()
                };
                state.powered = new != 0;
                state.output_strength = new;
                backend.evaluate_seed(root, None).unwrap();
            }
            assert_same_execution(&general, &isolated);
            assert_eq!(replay_count(&isolated), usize::from(old != new));
            assert_eq!(replay_count(&general), 0);
            assert!(
                !isolated
                    .trace()
                    .iter()
                    .any(|line| line.starts_with("wire discover ") || line.starts_with("wire heading "))
            );
            let deliveries: Vec<_> = isolated
                .callback_order
                .iter()
                .filter(|(position, _)| [LAMP, ABOVE].contains(position))
                .copied()
                .collect();
            let update = DeliveryKind::Update { direction: None };
            assert_eq!(
                deliveries,
                if old == new {
                    vec![]
                } else {
                    vec![(ABOVE, update), (LAMP, update), (ABOVE, update)]
                }
            );
        }
    }
}

fn set_native_lever(world: &mut PlotWorld, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(SOURCE) else {
        unreachable!()
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(SOURCE, Block::Lever { lever });
    redstone::update_surrounding_blocks(world, SOURCE);
    redstone::update_surrounding_blocks(world, SOURCE.offset(BlockFace::Bottom));
}

#[test]
fn singleton_replay_matches_native_callbacks_pending_ticks_and_reset_continuation() {
    let source = Block::Lever {
        lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
    };
    let mut native = world(source);
    let mut general_world = world(source);
    let mut isolated_world = world(source);
    let graph = draft(&native);
    let mut general = backend(graph.clone(), false);
    let mut isolated = backend(graph, true);
    let watched = [ROOT, LAMP, ABOVE];
    for powered in [true, true, false, true, false, false, true] {
        let start = isolated.observed_operations.len();
        let Block::Lever { lever } = native.get_block(SOURCE) else {
            unreachable!()
        };
        let changed = lever.powered != powered;
        let entries = capture_at(&watched, || {
            set_native_lever(&mut native, powered);
        });
        if changed {
            general.use_block(SOURCE).unwrap();
            isolated.use_block(SOURCE).unwrap();
        }
        assert_same_execution(&general, &isolated);
        assert_eq!(graph_operations_since(&isolated, start, &watched), native_operations(&entries, &watched));
        for _ in 0..2 {
            let start = isolated.observed_operations.len();
            let entries = capture_at(&watched, || native.tick_interpreted());
            general.tick().unwrap();
            isolated.tick().unwrap();
            assert_same_execution(&general, &isolated);
            assert_eq!(graph_operations_since(&isolated, start, &watched), native_operations(&entries, &watched));
            general.flush(&mut general_world, false);
            isolated.flush(&mut isolated_world, false);
            for position in watched.into_iter().chain([SOURCE]) {
                assert_eq!(general_world.get_block(position), native.get_block(position));
                assert_eq!(isolated_world.get_block(position), native.get_block(position));
            }
            assert_eq!(isolated.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
        }
    }
    assert!(replay_count(&isolated) > 0);
    set_native_lever(&mut native, false);
    general.use_block(SOURCE).unwrap();
    isolated.use_block(SOURCE).unwrap();
    assert_same_execution(&general, &isolated);
    assert!(!isolated.scheduled_ticks().is_empty(), "reset must preserve a pending lamp-off tick");
    general.reset(&mut general_world);
    isolated.reset(&mut isolated_world);
    for _ in 0..4 {
        native.tick_interpreted();
        general_world.tick_interpreted();
        isolated_world.tick_interpreted();
        for position in watched {
            assert_eq!(general_world.get_block(position), native.get_block(position));
            assert_eq!(isolated_world.get_block(position), native.get_block(position));
        }
        assert_eq!(
            isolated_world.scheduler().iter_entries().collect::<Vec<_>>(),
            native.scheduler().iter_entries().collect::<Vec<_>>()
        );
    }
}

#[test]
fn singleton_replay_keeps_outer_captures_and_order_during_nested_reentry() {
    let mut world = world(Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
    });
    world.set_block(LAMP, Block::from_name("copper_bulb").unwrap());
    let mut graph = draft(&world);
    let root = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(ROOT))
        .map(|index| NodeId::new(index as u32))
        .unwrap();
    let bulb = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(LAMP))
        .map(|index| NodeId::new(index as u32))
        .unwrap();
    let root_site = graph
        .sites
        .iter()
        .position(|site| site.position == ROOT)
        .map(|index| SiteId::new(index as u32))
        .unwrap();
    let bulb_site = graph
        .sites
        .iter()
        .position(|site| site.position == LAMP)
        .map(|index| SiteId::new(index as u32))
        .unwrap();
    let support = graph
        .sites
        .iter()
        .position(|site| site.position == ROOT.offset(BlockFace::Bottom))
        .map(|index| SiteId::new(index as u32))
        .unwrap();
    let route = RouteId::new(graph.routes.len() as u32);
    graph.routes.push(Route {
        source: bulb,
        deliveries: vec![RouteDelivery {
            target: root_site,
            kind: DeliveryKind::Update { direction: None },
            guard: GuardId::new(0),
            view: OperandView::Live,
            sequence: 0,
            receiver: Some(CallbackReceiver::Component(root)),
        }]
        .into_boxed_slice(),
    });
    graph.links.push(LinkSpec {
        from: bulb,
        to: root,
        kind: LinkKind::Update {
            direction: None,
            guard: GuardId::new(0),
        },
        callback: CallbackEdgeRef { route, sequence: 0 },
    });
    graph.nodes[bulb.index()].output_route = Some(route);
    // A synthetic conducted output makes the bulb's callback start a second seed.
    graph.power_links.push(PowerLink {
        from: bulb_site,
        from_port: PowerPort::Output,
        to: PowerTarget::Conductor {
            site: support,
            include_dust: false,
        },
        face: BlockFace::East,
        attenuation: 0,
        guard: GuardId::new(0),
    });
    let mut general = backend(graph.clone(), false);
    let mut isolated = backend(graph, true);
    for backend in [&mut general, &mut isolated] {
        let source = backend.positions[&SOURCE];
        let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
            unreachable!()
        };
        state.powered = true;
        state.output_strength = 1;
        backend.evaluate_seed(root, None).unwrap();
    }
    assert_eq!(isolated.wire_state(root).power, 15);
    assert_eq!(replay_count(&isolated), 2);
    assert_same_execution(&general, &isolated);
    assert!(isolated.node_state(bulb).powered);
}
