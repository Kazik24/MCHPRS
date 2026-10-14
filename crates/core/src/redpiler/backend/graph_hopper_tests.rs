//! Stationary inventory and enabled-state equivalence with the interpreter.
use super::*;
use crate::plot::PlotWorld;
use crate::redstone;
use crate::world::storage::Chunk;
use mchprs_blocks::BlockDirection;
use mchprs_blocks::blocks::{ComparatorMode, HopperFacing, Lever, LeverFace, RedstoneComparator};

const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
const COMPARATOR: BlockPos = BlockPos::new(8, 20, 8);

fn inventory(count: i8) -> BlockEntity {
    let items = if count == 0 {
        vec![]
    } else {
        (0..5)
            .map(|slot| {
                nbt::Value::Compound(std::collections::HashMap::from([
                    ("Slot".into(), nbt::Value::Byte(slot)),
                    ("Count".into(), nbt::Value::Byte(count)),
                    ("id".into(), nbt::Value::String("minecraft:redstone".into())),
                ]))
            })
            .collect()
    };
    BlockEntity::from_nbt(&std::collections::HashMap::from([
        ("id".into(), nbt::Value::String("minecraft:hopper".into())),
        ("Items".into(), nbt::Value::List(items)),
    ]))
    .unwrap()
}

fn world(count: i8, through_solid: bool, enabled: bool) -> (PlotWorld, BlockPos, BlockPos) {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    let rear = COMPARATOR.offset(BlockFace::South);
    let hopper = if through_solid { rear.offset(BlockFace::South) } else { rear };
    if through_solid {
        world.set_block(rear, Block::Stone {});
    }
    let lever = hopper.offset(BlockFace::East);
    let entity = inventory(count);
    world.set_block(
        COMPARATOR,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false),
        },
    );
    world.set_block_entity(COMPARATOR, BlockEntity::Comparator { output_strength: 0 });
    world.set_block(COMPARATOR.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        hopper,
        Block::Hopper {
            enabled,
            facing: HopperFacing::Down,
        },
    );
    world.set_block_entity(hopper, entity);
    for wire in [hopper.offset(BlockFace::Top), hopper.offset(BlockFace::Bottom)] {
        world.set_block(wire, Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(hopper.offset(BlockFace::Bottom).offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(lever.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        lever,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    (world, hopper, lever)
}

fn backend(world: &PlotWorld, level: usize) -> GraphBackend {
    let monitor = TaskMonitor::default();
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(level),
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(world, BOUNDS, vec![], &monitor, crate::redpiler::analysis::AnalysisLimits::default()).unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap()
}

#[test]
fn hopper_inventory_analog_and_enabled_history_match_interpreter_at_all_levels() {
    for level in 0..=2 {
        for (count, strength) in [(0, 0), (32, 8), (64, 15)] {
            for through_solid in [false, true] {
                for enabled in [false, true] {
                    let (mut graph_world, hopper, lever) = world(count, through_solid, enabled);
                    let (mut native, _, _) = world(count, through_solid, enabled);
                    let initial_entity = serde_json::to_value(graph_world.get_block_entity(hopper)).unwrap();
                    let mut backend = backend(&graph_world, level);
                    let hopper_node = backend.positions[&hopper];
                    assert_eq!(
                        backend.program.nodes()[hopper_node.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::Hopper)
                    );
                    assert_eq!(backend.node_state(hopper_node).output_strength, strength);
                    assert!(backend.program.nodes()[hopper_node.index()].output_route.is_none());
                    let site = backend.component_sites[hopper_node.index()].unwrap();
                    let resolved = backend.live_site(site);
                    assert_eq!(resolved.solid, graph_world.get_block(hopper).is_solid());
                    assert_eq!(resolved.transparent, graph_world.get_block(hopper).is_transparent());
                    assert_eq!(resolved.block.is_cube(), graph_world.get_block(hopper).is_cube());
                    let mut previous = false;
                    for tick in 0..100 {
                        let powered = (tick * 17 % 13) < 6;
                        if previous != powered {
                            backend.use_block(lever).unwrap();
                            let Block::Lever { lever: mut state } = native.get_block(lever) else {
                                unreachable!()
                            };
                            state.powered = powered;
                            native.set_block(lever, Block::Lever { lever: state });
                            redstone::update_surrounding_blocks(&mut native, lever);
                            redstone::update_surrounding_blocks(&mut native, lever.offset(BlockFace::Bottom));
                            previous = powered;
                        }
                        backend.flush(&mut graph_world, true);
                        assert_eq!(
                            graph_world.get_block(hopper),
                            native.get_block(hopper),
                            "level={level} count={count} far={through_solid} tick={tick}"
                        );
                        let comparator_node = backend.positions[&COMPARATOR];
                        backend
                            .update_component(comparator_node, DeliveryKind::Update { direction: None }, None)
                            .unwrap();
                        redstone::update(native.get_block(COMPARATOR), &mut native, COMPARATOR, None);
                        backend.tick().unwrap();
                        native.tick_interpreted();
                        backend.flush(&mut graph_world, false);
                        for pos in [COMPARATOR, hopper, lever, hopper.offset(BlockFace::Top), hopper.offset(BlockFace::Bottom)] {
                            assert_eq!(graph_world.get_block_raw(pos), native.get_block_raw(pos), "at {pos:?}, tick {tick}");
                            assert_eq!(
                                serde_json::to_value(graph_world.get_block_entity(pos)).unwrap(),
                                serde_json::to_value(native.get_block_entity(pos)).unwrap()
                            );
                        }
                        assert_eq!(serde_json::to_value(graph_world.get_block_entity(hopper)).unwrap(), initial_entity);
                        assert_eq!(backend.node_state(hopper_node).output_strength, strength);
                        assert_eq!(backend.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
                    }
                    backend.reset(&mut graph_world);
                    assert_eq!(graph_world.get_block(hopper), native.get_block(hopper));
                    assert_eq!(serde_json::to_value(graph_world.get_block_entity(hopper)).unwrap(), initial_entity);
                    for _ in 0..4 {
                        graph_world.tick_interpreted();
                        native.tick_interpreted();
                        assert_eq!(graph_world.get_block(hopper), native.get_block(hopper));
                    }
                }
            }
        }
    }
}

#[test]
fn hopper_captured_enabled_operand_preserves_native_no_write_case() {
    let (world, hopper, _) = world(64, false, true);
    let mut backend = backend(&world, 0);
    let node = backend.positions[&hopper];
    let site = backend.component_sites[node.index()].unwrap();
    let captured = backend.resolve_site(site, None).unwrap().observed();
    backend.set_node(node, true, 15, None, false).unwrap();
    backend
        .update_component(node, DeliveryKind::Update { direction: None }, Some(&captured))
        .unwrap();
    // Captured enabled=true already matches unpowered input: native performs no write.
    assert!(backend.node_state(node).powered);
    assert_eq!(backend.node_state(node).output_strength, 15);
    backend.update_component(node, DeliveryKind::Update { direction: None }, None).unwrap();
    assert!(!backend.node_state(node).powered);
    assert!(backend.scheduled_ticks().is_empty());
}

#[test]
fn hopper_blocking_a_piston_retains_live_contact_state() {
    let (mut world, hopper, lever) = world(32, false, true);
    world.set_block(hopper.offset(BlockFace::Top), Block::Air);
    let piston = hopper.offset(BlockFace::South);
    world.set_block(
        piston,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFace::North.into(),
                sticky: false,
                extended: false,
            },
        },
    );
    let mut backend = backend(&world, 2);
    let node = backend.positions[&hopper];
    let site = backend.component_sites[node.index()]
        .unwrap_or_else(|| SiteId::try_from_index(backend.program.sites().iter().position(|site| site.position == hopper).unwrap()).unwrap());
    assert!(matches!(backend.program.sites()[site.index()].source, SiteSource::Contact(_)));
    assert!(backend.program.resources().is_empty());
    backend.use_block(lever).unwrap();
    assert!(!matches!(backend.live_site(site).block, Block::Hopper { enabled: true, .. }));
    let observed = backend.resolve_site(site, None).unwrap().observed();
    assert!(matches!(observed.properties, ObservedProperties::Electrical { powered: true, .. }));
    backend.flush(&mut world, false);
    assert_eq!(backend.live_site(site).block, world.get_block(hopper));
    assert_eq!(crate::redstone::comparator::get_override(world.get_block(hopper), &world, hopper), 8);
}
