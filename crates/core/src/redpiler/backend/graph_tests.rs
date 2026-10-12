//! Graph backend differential and regression tests.
use super::*;
use crate::plot::PlotWorld;
use crate::plot::worldedit::{WorldEditClipboard, load_schematic};
use crate::redpiler::backend::ScheduledBlockTick;
use crate::redstone;
use crate::redstone::instant_piston_tests::capture_at;
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{
    Block, ButtonFace, ComparatorMode, Lever, LeverFace, RedstoneComparator, RedstoneObserver,
    RedstoneRepeater, RedstoneWire, RedstoneWireSide, StoneButton,
};
use mchprs_blocks::{BlockDirection, BlockFacing};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const BOUNDS: (BlockPos, BlockPos) = (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15));
const LEVER_POS: BlockPos = BlockPos::new(5, 20, 5);
const NEXT_POS: BlockPos = BlockPos::new(6, 20, 5);

fn empty_world() -> PlotWorld {
    PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default())
}

fn lever(powered: bool) -> Block {
    Block::Lever {
        lever: Lever::new(LeverFace::Floor, BlockDirection::North, powered),
    }
}

fn set_native_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    set_native_lever_with_support(world, position, powered, BlockFace::Bottom);
}

fn set_native_lever_with_support(
    world: &mut PlotWorld,
    position: BlockPos,
    powered: bool,
    support: BlockFace,
) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("expected lever at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    redstone::update_surrounding_blocks(world, position);
    redstone::update_surrounding_blocks(world, position.offset(support));
}

fn graph(world: &PlotWorld, ticks: Vec<TickEntry>) -> GraphBackend {
    GraphBackend::compile(world, BOUNDS, ticks, Arc::default()).unwrap()
}

fn state_at(backend: &GraphBackend, position: BlockPos) -> NodeState {
    backend.node_state(backend.positions[&position])
}

fn ticks_at(
    position: BlockPos,
    block: Block,
    count: usize,
    priority: TickPriority,
) -> Vec<TickEntry> {
    (0..count)
        .map(|_| TickEntry {
            ticks_left: 0,
            tick_priority: priority,
            pos: position,
            block_type: Some(block.registry_id()),
        })
        .collect()
}

fn test_profile(block: Block, electrical: ProfileElectrical) -> Profile {
    Profile {
        raw_state: block.get_id(),
        registry_type: block.registry_id(),
        solid: block.is_solid(),
        transparent: block.is_transparent(),
        cube: block.is_cube(),
        electrical,
    }
}

fn contact_resource_backend() -> GraphBackend {
    let resource = ResourceId::new(0);
    let first = ContactId::new(0);
    let second = ContactId::new(1);
    let first_at = GuardId::new(1);
    let enabled = GuardId::new(2);
    let at_first_and_enabled = GuardId::new(3);
    let second_at = GuardId::new(4);
    let at_second_and_enabled = GuardId::new(6);
    let redstone = ProfileId::new(1);
    let air = ProfileId::new(0);
    let guards = vec![
        Guard::Always,
        Guard::State(StateTest::ResourceAt {
            resource,
            contact: first,
        }),
        Guard::State(StateTest::ResourceEnabled(resource)),
        Guard::All(vec![first_at, enabled].into_boxed_slice()),
        Guard::State(StateTest::ResourceAt {
            resource,
            contact: second,
        }),
        Guard::State(StateTest::ResourceEnabled(resource)),
        Guard::All(vec![second_at, GuardId::new(5)].into_boxed_slice()),
        Guard::Not(at_first_and_enabled),
        Guard::Not(at_second_and_enabled),
    ];
    let (program, state) = CircuitDraft {
        nodes: vec![first, second]
            .into_iter()
            .enumerate()
            .map(|(index, contact)| CircuitNodeDraft {
                position: Some(BlockPos::new(index as i32, 0, 0)),
                raw_state: None,
                kind: CircuitNodeKind::Contact(contact),
                state: ComponentState::Specialized,
                output_route: None,
            })
            .collect(),
        links: vec![],
        power_links: vec![],
        comparator_inputs: vec![],
        profiles: vec![
            test_profile(Block::Air, ProfileElectrical::None),
            test_profile(Block::RedstoneBlock, ProfileElectrical::RedstoneSource),
            test_profile(
                Block::RedstoneComparator {
                    comparator: RedstoneComparator::new(
                        BlockDirection::South,
                        ComparatorMode::Compare,
                        true,
                    ),
                },
                ProfileElectrical::Component,
            ),
        ],
        guards,
        contacts: vec![
            ContactSpec {
                node: NodeId::new(0),
                position: BlockPos::new(0, 0, 0),
                variants: vec![
                    ContactVariant {
                        when: at_first_and_enabled,
                        profile: redstone,
                        receiver: None,
                    },
                    ContactVariant {
                        when: GuardId::new(7),
                        profile: air,
                        receiver: None,
                    },
                ]
                .into_boxed_slice(),
            },
            ContactSpec {
                node: NodeId::new(1),
                position: BlockPos::new(1, 0, 0),
                variants: vec![
                    ContactVariant {
                        when: at_second_and_enabled,
                        profile: redstone,
                        receiver: None,
                    },
                    ContactVariant {
                        when: GuardId::new(8),
                        profile: air,
                        receiver: None,
                    },
                ]
                .into_boxed_slice(),
            },
        ],
        routes: vec![],
        wires: vec![],
        sites: vec![
            SiteSpec {
                position: BlockPos::new(1, 0, 0),
                source: SiteSource::Contact(second),
                source_strength: 0,
            },
            SiteSpec {
                position: BlockPos::new(2, 0, 0),
                source: SiteSource::Constant(ProfileId::new(2)),
                source_strength: 7,
            },
        ],
        pistons: vec![],
        resources: vec![ResourceSpec {
            material: redstone,
            contacts: vec![first, second].into_boxed_slice(),
        }],
        initial_pistons: vec![],
        initial_resources: vec![ResourceState {
            at: first,
            enabled: true,
            revision: 0,
        }],
    }
    .lower()
    .unwrap();
    GraphBackend::new(program, state, 0).unwrap()
}

fn backend_with_unrelated_power_links(unrelated: usize) -> GraphBackend {
    let node = NodeId::new(0);
    let source = SiteId::new(0);
    let conductor = SiteId::new(1);
    let mut power_links = vec![
        PowerLink {
            from: source,
            from_port: PowerPort::Output,
            to: PowerTarget::Input {
                node,
                port: InputPort::Main,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(0),
        },
        PowerLink {
            from: conductor,
            from_port: PowerPort::Output,
            to: PowerTarget::Conductor {
                site: conductor,
                include_dust: false,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(0),
        },
    ];
    for _ in 0..unrelated {
        power_links.push(PowerLink {
            from: source,
            from_port: PowerPort::Output,
            to: PowerTarget::Input {
                node,
                port: InputPort::Side,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(0),
        });
        power_links.push(PowerLink {
            from: source,
            from_port: PowerPort::Output,
            to: PowerTarget::Conductor {
                site: conductor,
                include_dust: true,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(0),
        });
    }
    let (program, state) = CircuitDraft {
        nodes: vec![CircuitNodeDraft {
            position: Some(BlockPos::new(0, 0, 0)),
            raw_state: Some(Block::RedstoneLamp { lit: false }.get_id()),
            kind: CircuitNodeKind::Electrical(NodeType::Lamp),
            state: ComponentState::Electrical(NodeState::default()),
            output_route: None,
        }],
        links: vec![],
        power_links,
        comparator_inputs: vec![],
        profiles: vec![test_profile(Block::Stone {}, ProfileElectrical::None)],
        guards: vec![Guard::Always],
        contacts: vec![],
        routes: vec![],
        wires: vec![],
        sites: vec![
            SiteSpec {
                position: BlockPos::new(1, 0, 0),
                source: SiteSource::Constant(ProfileId::new(0)),
                source_strength: 15,
            },
            SiteSpec {
                position: BlockPos::new(2, 0, 0),
                source: SiteSource::Constant(ProfileId::new(0)),
                source_strength: 13,
            },
        ],
        pistons: vec![],
        resources: vec![],
        initial_pistons: vec![],
        initial_resources: vec![],
    }
    .lower()
    .unwrap();
    GraphBackend::new(program, state, 0).unwrap()
}

#[test]
fn graph_route_powers_lamp_synchronously_and_flush_only_projects_state() {
    let lamp_pos = LEVER_POS.offset(BlockFace::East);
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(LEVER_POS, lever(false));
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = graph(&graph_world, vec![]);
    let lever_node = backend.positions[&LEVER_POS];
    let lamp_node = backend.positions[&lamp_pos];

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    assert!(state_at(&backend, lamp_pos).powered);
    assert!(matches!(
        native.get_block(lamp_pos),
        Block::RedstoneLamp { lit: true }
    ));

    let trace = backend.trace();
    let source_change = trace
        .iter()
        .position(|item| item.starts_with(&format!("state {lever_node:?}")))
        .unwrap();
    let lamp_change = trace
        .iter()
        .position(|item| item.starts_with(&format!("state {lamp_node:?}")))
        .unwrap();
    let delivery = trace
        .iter()
        .position(|item| item.starts_with("route "))
        .unwrap();
    assert!(
        source_change < delivery && delivery < lamp_change,
        "{trace:#?}"
    );

    let state = backend.state.clone();
    let scheduled = backend.scheduled_ticks();
    let pending = backend.pending.clone();
    let clock = backend.clock;
    backend.flush(&mut graph_world);
    assert_eq!(backend.state, state);
    assert_eq!(backend.scheduled_ticks(), scheduled);
    assert_eq!(backend.pending, pending);
    assert_eq!(backend.clock.logical_tick, clock.logical_tick);
    assert_eq!(backend.clock.phase, clock.phase);
    assert_eq!(backend.clock.scheduled_advanced, clock.scheduled_advanced);
    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
}

#[test]
fn dust_output_uses_regulated_sides_from_compiled_neighbors() {
    let wire_pos = LEVER_POS.offset(BlockFace::East);
    let lamp_pos = wire_pos.offset(BlockFace::East);
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(LEVER_POS, lever(false));
        world.set_block(wire_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            wire_pos,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = graph(&graph_world, vec![]);
    assert!(
        matches!(backend.node_block(backend.positions[&wire_pos]), Block::RedstoneWire { wire }
        if wire.north == RedstoneWireSide::None && wire.south == RedstoneWireSide::None
            && wire.east == RedstoneWireSide::None && wire.west == RedstoneWireSide::None)
    );

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    backend.flush(&mut graph_world);

    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert!(matches!(
        graph_world.get_block(lamp_pos),
        Block::RedstoneLamp { lit: true }
    ));
    assert!(
        matches!(graph_world.get_block(wire_pos), Block::RedstoneWire { wire }
        if wire.power == 15 && wire.north == RedstoneWireSide::None
            && wire.south == RedstoneWireSide::None && wire.east == RedstoneWireSide::None
            && wire.west == RedstoneWireSide::None)
    );
}

#[test]
fn external_torch_boundary_supplies_power_to_selected_lamp() {
    let lamp_pos = BlockPos::new(10, 20, 5);
    let torch_pos = lamp_pos.offset(BlockFace::East);
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(10, 31, 10));
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
        world.set_block(torch_pos, Block::RedstoneTorch { lit: true });
    }
    let mut backend = GraphBackend::compile(&graph_world, bounds, vec![], Arc::default()).unwrap();
    let lamp = backend.positions[&lamp_pos];
    assert!(!backend.positions.contains_key(&torch_pos));
    assert_eq!(backend.input_strength(lamp, InputPort::Main), 15);

    backend.update_lamp(lamp, None).unwrap();
    redstone::update(native.get_block(lamp_pos), &mut native, lamp_pos, None);
    backend.flush(&mut graph_world);
    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert!(matches!(
        graph_world.get_block(lamp_pos),
        Block::RedstoneLamp { lit: true }
    ));
}

#[test]
fn dust_strong_output_uses_regulated_sides_through_a_conductor() {
    let wire_pos = LEVER_POS.offset(BlockFace::East);
    let conductor_pos = wire_pos.offset(BlockFace::East);
    let lamp_pos = conductor_pos.offset(BlockFace::East);
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(LEVER_POS, lever(false));
        world.set_block(wire_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            wire_pos,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
        world.set_block(conductor_pos, Block::Stone {});
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = graph(&graph_world, vec![]);

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    backend.flush(&mut graph_world);

    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert!(matches!(
        graph_world.get_block(lamp_pos),
        Block::RedstoneLamp { lit: true }
    ));
}

#[test]
fn wire_seed_and_turbo_rise_and_fall_match_native_order() {
    let lever_pos = BlockPos::new(4, 20, 6);
    let wire_positions: Vec<_> = (5..=10).map(|x| BlockPos::new(x, 20, 6)).collect();
    let lamp_pos = BlockPos::new(10, 20, 7);
    let observed: Vec<_> = wire_positions.iter().copied().chain([lamp_pos]).collect();
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(lever_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(lever_pos, lever(false));
        for position in &wire_positions {
            world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                *position,
                Block::RedstoneWire {
                    wire: Default::default(),
                },
            );
        }
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = graph(&graph_world, vec![]);

    for powered in [true, false] {
        let graph_start = backend.callback_order.len();
        backend.use_block(lever_pos).unwrap();
        let native_trace = capture_at(&observed, || {
            set_native_lever(&mut native, lever_pos, powered)
        });
        let native_callbacks: Vec<_> = native_trace
            .iter()
            .filter(|entry| entry["kind"] == "callback")
            .map(|entry| {
                (
                    serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                    serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
                )
            })
            .collect();
        let graph_callbacks: Vec<_> = backend.callback_order[graph_start..]
            .iter()
            .filter(|(position, _)| observed.contains(position))
            .map(|(position, kind)| {
                let DeliveryKind::Update { direction } = kind else {
                    unreachable!()
                };
                (*position, *direction)
            })
            .collect();

        backend.flush(&mut graph_world);
        assert_eq!(
            graph_callbacks,
            native_callbacks,
            "graph trace: {:#?}",
            backend.trace()
        );
        for position in &wire_positions {
            assert_eq!(
                graph_world.get_block(*position),
                native.get_block(*position),
                "wire at {position:?} after powered={powered}; trace: {:#?}",
                backend.trace()
            );
        }
        assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
        for prefix in [
            "wire seed ",
            "wire turbo start ",
            "wire discover ",
            "wire heading ",
            "wire query ",
            "wire strength ",
            "wire deliver ",
        ] {
            assert!(
                backend
                    .trace()
                    .iter()
                    .any(|entry| entry.starts_with(prefix)),
                "missing trace operation {prefix:?}: {:#?}",
                backend.trace()
            );
        }
    }
}

#[test]
fn wire_seed_with_unchanged_power_does_not_start_a_turbo_walk() {
    let lever_pos = BlockPos::new(7, 20, 8);
    let wire_pos = lever_pos.offset(BlockFace::East);
    let wire = RedstoneWire {
        power: 15,
        ..Default::default()
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(lever_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(lever_pos, lever(true));
        world.set_block(wire_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(wire_pos, Block::RedstoneWire { wire });
    }
    let mut backend = graph(&graph_world, vec![]);
    let node = backend.positions[&wire_pos];
    backend
        .update_component(node, DeliveryKind::Update { direction: None }, None)
        .unwrap();
    redstone::update(native.get_block(wire_pos), &mut native, wire_pos, None);

    assert_eq!(backend.wire_state(node), wire);
    assert_eq!(graph_world.get_block(wire_pos), native.get_block(wire_pos));
    assert!(
        backend
            .trace()
            .iter()
            .any(|entry| entry.starts_with("wire seed "))
    );
    assert!(
        !backend
            .trace()
            .iter()
            .any(|entry| entry.starts_with("wire turbo start "))
    );
}

#[test]
fn stale_wire_sides_and_strength_remain_untouched_until_a_rule_changes_them() {
    let wire_pos = BlockPos::new(8, 20, 8);
    let wire = RedstoneWire {
        north: RedstoneWireSide::Up,
        south: RedstoneWireSide::None,
        east: RedstoneWireSide::Side,
        west: RedstoneWireSide::None,
        power: 7,
    };
    let mut world = empty_world();
    world.set_block(wire_pos.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(wire_pos, Block::RedstoneWire { wire });
    let mut backend = graph(&world, vec![]);

    backend.flush(&mut world);

    assert_eq!(world.get_block(wire_pos), Block::RedstoneWire { wire });
    assert_eq!(backend.wire_state(backend.positions[&wire_pos]), wire);
}

#[test]
fn wire_shape_callback_regulates_sides_then_uses_native_neighbor_route() {
    let wire_pos = BlockPos::new(8, 20, 8);
    let source_pos = wire_pos.offset(BlockFace::East);
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(wire_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            wire_pos,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
        world.set_block(source_pos, Block::RedstoneBlock {});
    }
    let mut backend = graph(&graph_world, vec![]);
    let node = backend.positions[&wire_pos];
    let native_trace = capture_at(&[wire_pos], || {
        crate::interaction::change(
            native.get_block(wire_pos),
            &mut native,
            wire_pos,
            BlockFace::West,
        );
    });
    backend
        .update_component(
            node,
            DeliveryKind::Shape {
                direction: BlockFace::West,
            },
            None,
        )
        .unwrap();
    backend.flush(&mut graph_world);

    let native_callbacks: Vec<_> = native_trace
        .iter()
        .filter(|entry| entry["kind"] == "callback")
        .map(|entry| serde_json::from_value(entry["data"]["dir"].clone()).unwrap())
        .collect();
    let graph_callbacks: Vec<_> = backend
        .callback_order
        .iter()
        .filter_map(|(position, kind)| {
            (*position == wire_pos).then_some(match kind {
                DeliveryKind::Update { direction } => *direction,
                DeliveryKind::Shape { .. } => unreachable!(),
            })
        })
        .collect();
    assert_eq!(graph_callbacks, native_callbacks);
    assert_eq!(graph_world.get_block(wire_pos), native.get_block(wire_pos));
    assert!(matches!(
        graph_world.get_block(wire_pos),
        Block::RedstoneWire { wire } if wire.power == 15 && wire.east == RedstoneWireSide::Side
    ));
}

#[test]
fn wire_turbo_walk_matches_native_across_ascent_and_descent() {
    let lever_pos = BlockPos::new(4, 20, 6);
    let wires = [
        BlockPos::new(5, 20, 6),
        BlockPos::new(6, 21, 6),
        BlockPos::new(7, 21, 6),
        BlockPos::new(8, 20, 6),
        BlockPos::new(9, 20, 6),
    ];
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(lever_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(lever_pos, lever(false));
        world.set_block(BlockPos::new(6, 20, 6), Block::Stone {});
        world.set_block(BlockPos::new(7, 20, 6), Block::Stone {});
        for position in wires {
            world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(
                position,
                Block::RedstoneWire {
                    wire: Default::default(),
                },
            );
        }
    }
    let mut backend = graph(&graph_world, vec![]);
    backend.use_block(lever_pos).unwrap();
    set_native_lever(&mut native, lever_pos, true);
    backend.flush(&mut graph_world);

    for position in wires {
        assert_eq!(
            graph_world.get_block(position),
            native.get_block(position),
            "wire at {position:?}"
        );
    }
    assert!(matches!(
        graph_world.get_block(wires[1]),
        Block::RedstoneWire { wire } if wire.power > 0
    ));
    assert!(matches!(
        graph_world.get_block(wires[3]),
        Block::RedstoneWire { wire } if wire.power > 0
    ));
}

#[test]
fn wire_turbo_heading_orders_match_native_for_all_horizontal_directions() {
    for direction in [
        BlockFace::North,
        BlockFace::South,
        BlockFace::East,
        BlockFace::West,
    ] {
        let first_wire = BlockPos::new(8, 20, 8);
        let wires: Vec<_> = std::iter::successors(Some(first_wire), |position| {
            Some(position.offset(direction))
        })
        .take(6)
        .collect();
        let lever_pos = first_wire.offset(direction.opposite());
        let mut native = empty_world();
        let mut graph_world = empty_world();
        for world in [&mut native, &mut graph_world] {
            world.set_block(lever_pos.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(lever_pos, lever(false));
            for position in &wires {
                world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(
                    *position,
                    Block::RedstoneWire {
                        wire: Default::default(),
                    },
                );
            }
        }
        let mut backend = graph(&graph_world, vec![]);
        backend.use_block(lever_pos).unwrap();
        let native_trace = capture_at(&wires, || set_native_lever(&mut native, lever_pos, true));
        let native_callbacks: Vec<_> = native_trace
            .iter()
            .filter(|entry| entry["kind"] == "callback")
            .map(|entry| {
                (
                    serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                    serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
                )
            })
            .collect();
        let graph_callbacks: Vec<_> = backend
            .callback_order
            .iter()
            .filter(|(position, _)| wires.contains(position))
            .map(|(position, kind)| {
                let DeliveryKind::Update { direction } = kind else {
                    unreachable!()
                };
                (*position, *direction)
            })
            .collect();

        assert_eq!(
            graph_callbacks,
            native_callbacks,
            "direction={direction:?}; graph trace: {:#?}",
            backend.trace()
        );
        backend.flush(&mut graph_world);
        for position in &wires {
            assert_eq!(
                graph_world.get_block(*position),
                native.get_block(*position),
                "direction={direction:?}, wire={position:?}"
            );
        }
    }
}

#[test]
fn wire_reads_live_strong_power_through_a_compiled_conductor() {
    let conductor_pos = BlockPos::new(8, 20, 8);
    let lever_pos = conductor_pos.offset(BlockFace::Top);
    let wire_pos = conductor_pos.offset(BlockFace::East);
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(conductor_pos, Block::Stone {});
        world.set_block(lever_pos, lever(false));
        world.set_block(wire_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            wire_pos,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
    }
    let mut backend = graph(&graph_world, vec![]);

    for powered in [true, false] {
        backend.use_block(lever_pos).unwrap();
        set_native_lever(&mut native, lever_pos, powered);
        backend.flush(&mut graph_world);
        assert_eq!(
            graph_world.get_block(wire_pos),
            native.get_block(wire_pos),
            "powered={powered}; graph trace: {:#?}",
            backend.trace()
        );
    }
    assert!(backend.trace().iter().any(|entry| {
        entry.starts_with("wire seed ")
            && entry.contains("operands=live-sites/live-strong")
            && entry.contains("block=15")
    }));
}

#[test]
fn compiled_wire_side_sites_match_native_vertical_and_horizontal_rules() {
    let wire_pos = BlockPos::new(8, 20, 8);
    let north = wire_pos.offset(BlockFace::North);
    let east = wire_pos.offset(BlockFace::East);
    let west = wire_pos.offset(BlockFace::West);
    let mut world = empty_world();
    world.set_block(north, Block::Stone {});
    world.set_block(
        north.offset(BlockFace::Top),
        Block::RedstoneWire {
            wire: Default::default(),
        },
    );
    world.set_block(
        east.offset(BlockFace::Bottom),
        Block::RedstoneWire {
            wire: Default::default(),
        },
    );
    world.set_block(west, Block::RedstoneBlock {});

    let sites =
        crate::world::wire_cache::positions(wire_pos).map(|position| world.get_block(position));
    let wire = RedstoneWire::default();
    let compiled = crate::redstone::wire::get_raw_sides_from_neighbors(wire, &sites);
    let native = crate::redstone::wire::get_raw_sides_from(wire, wire_pos, |position| {
        world.get_block(position)
    });

    assert_eq!(compiled, native);
    assert_eq!(compiled.north, RedstoneWireSide::Up);
    assert_eq!(compiled.east, RedstoneWireSide::Side);
    assert_eq!(compiled.south, RedstoneWireSide::None);
    assert_eq!(compiled.west, RedstoneWireSide::Side);
}

#[test]
fn bulb_callback_starts_a_nested_wire_walk_and_outer_order_resumes() {
    let lever_pos = BlockPos::new(5, 20, 5);
    let outer_wire = lever_pos.offset(BlockFace::East);
    let bulb_pos = outer_wire.offset(BlockFace::South);
    let inner_wire = bulb_pos.offset(BlockFace::East);
    let inner_source = inner_wire.offset(BlockFace::North);
    let continuation = inner_wire.offset(BlockFace::East);
    let bulb_source = bulb_pos.offset(BlockFace::South);
    let observed = [
        lever_pos,
        outer_wire,
        bulb_pos,
        inner_wire,
        inner_source,
        continuation,
    ];
    let mut native = empty_world();
    let mut graph_world = empty_world();
    let bulb = Block::from_name("copper_bulb").unwrap();
    for position in [lever_pos, outer_wire, inner_wire, continuation] {
        for world in [&mut native, &mut graph_world] {
            world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        }
    }
    for world in [&mut native, &mut graph_world] {
        world.set_block(lever_pos, lever(false));
        world.set_block(
            outer_wire,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
        world.set_block(
            inner_wire,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
        world.set_block(
            continuation,
            Block::RedstoneWire {
                wire: Default::default(),
            },
        );
        world.set_block(bulb_pos, bulb);
        world.set_block(inner_source, lever(false));
        world.set_block(bulb_source, Block::RedstoneBlock);
    }
    let mut backend = graph(&graph_world, vec![]);
    let inner_source_node = backend.positions[&inner_source];
    let outer_node = backend.positions[&outer_wire];
    let inner_node = backend.positions[&inner_wire];
    let continuation_node = backend.positions[&continuation];

    backend
        .set_node(inner_source_node, true, 15, None, false)
        .unwrap();
    native.set_block(inner_source, lever(true));

    backend.use_block(lever_pos).unwrap();
    let native_trace = capture_at(&observed, || set_native_lever(&mut native, lever_pos, true));
    let native_callbacks: Vec<_> = native_trace
        .iter()
        .filter(|entry| entry["kind"] == "callback")
        .map(|entry| {
            (
                serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
            )
        })
        .collect();
    let graph_callbacks: Vec<_> = backend
        .callback_order
        .iter()
        .filter(|(position, _)| observed.contains(position))
        .map(|(position, kind)| {
            let DeliveryKind::Update { direction } = kind else {
                unreachable!()
            };
            (*position, *direction)
        })
        .collect();
    assert_eq!(graph_callbacks, native_callbacks, "{:#?}", backend.trace());

    let bulb_callback_index = backend
        .callback_order
        .iter()
        .position(|(position, _)| *position == bulb_pos)
        .unwrap();
    let inner_callback_index = backend
        .callback_order
        .iter()
        .enumerate()
        .skip(bulb_callback_index + 1)
        .find_map(|(index, (position, _))| (*position == inner_wire).then_some(index))
        .unwrap();
    assert!(
        bulb_callback_index < inner_callback_index,
        "{:#?}",
        backend.callback_order
    );

    let native_nested_callback = native_trace
        .iter()
        .position(|entry| {
            entry["kind"] == "callback"
                && entry["data"]["pos"] == serde_json::to_value(inner_wire).unwrap()
                && entry["data"]["parents"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::to_value(bulb_pos).unwrap())
        })
        .unwrap_or_else(|| panic!("bulb did not notify the inner wire: {native_trace:#?}"));
    let native_nested_strength = native_trace
        .iter()
        .position(|entry| {
            entry["kind"] == "wire_strength"
                && serde_json::from_value::<BlockPos>(entry["data"]["pos"].clone()).unwrap()
                    == continuation
        })
        .unwrap_or_else(|| {
            panic!("nested walk did not write the continuation wire: {native_trace:#?}")
        });
    assert!(
        native_nested_callback < native_nested_strength,
        "{native_trace:#?}"
    );
    assert_eq!(native_trace[native_nested_strength]["data"]["old"], 0);
    assert_eq!(native_trace[native_nested_strength]["data"]["new"], 14);

    let starts: Vec<_> = backend
        .trace()
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            line.starts_with("wire turbo start ")
                .then_some((index, line))
        })
        .collect();
    let ends: Vec<_> = backend
        .trace()
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.starts_with("wire turbo end ").then_some((index, line)))
        .collect();
    assert_eq!(starts.len(), 2, "{:#?}", backend.trace());
    assert_eq!(ends.len(), 2, "{:#?}", backend.trace());
    assert!(starts[0].1.contains(&format!("{outer_node:?}")));
    assert!(starts[1].1.contains(&format!("{inner_node:?}")));
    assert!(ends[0].1.contains(&format!("{inner_node:?}")));
    assert!(ends[1].1.contains(&format!("{outer_node:?}")));
    assert!(
        starts[0].0 < starts[1].0 && starts[1].0 < ends[0].0 && ends[0].0 < ends[1].0,
        "{:#?}",
        backend.trace()
    );
    assert!(
        backend.trace()[ends[0].0 + 1..ends[1].0]
            .iter()
            .any(|line| line.starts_with("wire deliver "))
    );
    assert!(backend.trace().iter().any(|line| {
        line.starts_with(&format!("wire strength {continuation_node:?}"))
            && line.ends_with("0 -> 14")
    }));

    backend.flush(&mut graph_world);
    for position in [outer_wire, inner_wire, continuation, bulb_pos] {
        assert_eq!(graph_world.get_block(position), native.get_block(position));
    }
    assert!(
        matches!(graph_world.get_block(outer_wire), Block::RedstoneWire { wire } if wire.power == 15)
    );
    assert!(
        matches!(graph_world.get_block(inner_wire), Block::RedstoneWire { wire } if wire.power == 15)
    );
    assert!(
        matches!(graph_world.get_block(continuation), Block::RedstoneWire { wire } if wire.power == 14)
    );
    assert_eq!(
        graph_world.get_block(bulb_pos).copper_bulb_state(),
        Some((true, true))
    );
}

#[test]
fn observer_filters_direction_suppresses_pending_and_uses_two_tick_edges() {
    let observer_pos = NEXT_POS;
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(LEVER_POS, lever(false));
        world.set_block(
            observer_pos,
            Block::Observer {
                observer: RedstoneObserver {
                    facing: BlockFacing::West,
                    powered: false,
                },
            },
        );
    }
    let mut backend = graph(&graph_world, vec![]);
    let observer = backend.positions[&observer_pos];
    backend
        .update_component(
            observer,
            DeliveryKind::Update {
                direction: Some(BlockFace::North),
            },
            None,
        )
        .unwrap();
    assert!(
        !backend.is_pending(observer),
        "wrong watched face must be ignored"
    );

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    assert!(backend.is_pending(observer));
    assert!(native.pending_tick_at(observer_pos));
    backend
        .update_component(
            observer,
            DeliveryKind::Update {
                direction: Some(BlockFace::West),
            },
            None,
        )
        .unwrap();
    assert_eq!(
        backend.scheduled_ticks().len(),
        1,
        "a duplicate callback is suppressed"
    );

    for (tick, expected_powered) in [(1, false), (2, true), (3, true), (4, false)] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world);
        assert_eq!(
            state_at(&backend, observer_pos).powered,
            expected_powered,
            "tick {tick}"
        );
        assert_eq!(
            graph_world.get_block(observer_pos),
            native.get_block(observer_pos),
            "tick {tick}"
        );
        assert_eq!(
            backend.clock.logical_tick,
            native.piston_state().logical_tick
        );
    }

    let mut recovery_world = empty_world();
    recovery_world.set_block(
        observer_pos,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::West,
                powered: true,
            },
        },
    );
    let mut recovery = graph(&recovery_world, vec![]);
    let recovery_node = recovery.positions[&observer_pos];
    recovery
        .update_component(
            recovery_node,
            DeliveryKind::Update { direction: None },
            None,
        )
        .unwrap();
    assert!(
        recovery.is_pending(recovery_node),
        "powered/None recovery schedules the off edge"
    );
}

#[test]
fn shape_delivery_does_not_sample_observers() {
    let source = NEXT_POS;
    let away = source.offset(BlockFace::West);
    let toward = source.offset(BlockFace::East);
    let observer = |facing| Block::Observer {
        observer: RedstoneObserver {
            facing,
            powered: false,
        },
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(source, Block::Stone {});
        world.set_block(away, observer(BlockFacing::West));
        world.set_block(toward, observer(BlockFacing::West));
    }
    let mut backend = graph(&graph_world, vec![]);

    for (position, shape_direction, update_direction) in [
        (away, BlockFace::West, BlockFace::East),
        (toward, BlockFace::East, BlockFace::West),
    ] {
        let node = backend.positions[&position];
        crate::interaction::change(
            native.get_block(position),
            &mut native,
            position,
            shape_direction,
        );
        backend
            .update_component(
                node,
                DeliveryKind::Shape {
                    direction: shape_direction,
                },
                None,
            )
            .unwrap();
        assert!(!backend.is_pending(node));
        assert!(!native.pending_tick_at(position));

        redstone::update(
            native.get_block(position),
            &mut native,
            position,
            Some(update_direction),
        );
        backend
            .update_component(
                node,
                DeliveryKind::Update {
                    direction: Some(update_direction),
                },
                None,
            )
            .unwrap();
        assert_eq!(backend.is_pending(node), native.pending_tick_at(position));
    }
    assert!(!backend.is_pending(backend.positions[&away]));
    assert!(backend.is_pending(backend.positions[&toward]));
}

#[test]
fn imported_ticks_keep_duplicates_priority_fifo_and_reject_stale_types() {
    let first = BlockPos::new(5, 20, 5);
    let second = BlockPos::new(8, 20, 5);
    let button = Block::StoneButton {
        button: StoneButton::new(ButtonFace::Floor, BlockDirection::North, true),
    };
    let entries = vec![
        TickEntry {
            pos: second,
            block_type: Some(button.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: first,
            block_type: Some(button.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: second,
            block_type: Some(button.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: second,
            block_type: Some(Block::Air.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: first,
            block_type: Some(button.registry_id()),
            ticks_left: 2,
            tick_priority: TickPriority::Normal,
        },
    ];
    let scheduler = entries
        .clone()
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(first, button);
    native.set_block(second, button);
    let mut backend = graph(&native, entries);
    assert_eq!(
        backend.scheduled_ticks().len(),
        5,
        "duplicates and stale typed work are imported"
    );

    backend.tick().unwrap();
    native.tick_interpreted();
    assert_eq!(backend.scheduled_ticks().len(), 5);
    backend.tick().unwrap();
    native.tick_interpreted();
    assert!(backend.scheduled_ticks().is_empty());
    assert!(matches!(native.get_block(first), Block::StoneButton { button } if !button.powered));
    assert!(matches!(native.get_block(second), Block::StoneButton { button } if !button.powered));

    let dispatches: Vec<_> = backend
        .trace()
        .iter()
        .filter_map(|line| {
            backend.positions.iter().find_map(|(position, node)| {
                (line == &format!("dispatch {node:?}")).then_some(*position)
            })
        })
        .collect();
    assert_eq!(dispatches, [second, first, second, first]);
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| line.starts_with("stale "))
    );
    assert_eq!(
        backend
            .trace()
            .iter()
            .filter(|line| line.as_str() == "clock.advance")
            .count(),
        2
    );
}

#[test]
fn imported_wire_tick_is_consumed_as_a_native_noop() {
    let position = BlockPos::new(8, 20, 5);
    let block = Block::RedstoneWire {
        wire: Default::default(),
    };
    let entry = TickEntry {
        pos: position,
        block_type: Some(block.registry_id()),
        ticks_left: 2,
        tick_priority: TickPriority::High,
    };
    let scheduler = vec![entry.clone()]
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(position, block);
    let mut backend = graph(&native, vec![entry.clone()]);
    let node = backend.positions[&position];
    assert_eq!(backend.scheduled_ticks(), [entry]);

    backend.tick().unwrap();
    native.tick_interpreted();
    assert_eq!(backend.scheduled_ticks().len(), 1);
    assert_eq!(backend.scheduled_ticks()[0].ticks_left, 1);
    backend.tick().unwrap();
    native.tick_interpreted();

    assert!(backend.scheduled_ticks().is_empty());
    assert_eq!(backend.node_block(node), native.get_block(position));
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| line == &format!("dispatch {node:?}"))
    );
}

#[test]
fn imported_tick_validation_rejects_out_of_ring_and_invalid_zero_delay() {
    let mut world = empty_world();
    world.set_block(LEVER_POS, lever(false));
    let mut backend = graph(&world, vec![]);
    let tick = |ticks_left, tick_priority| TickEntry {
        pos: LEVER_POS,
        block_type: Some(lever(false).registry_id()),
        ticks_left,
        tick_priority,
    };

    assert!(matches!(
        backend.import_ticks(
            &world,
            vec![tick(SCHEDULER_BUCKETS as u32, TickPriority::Normal)]
        ),
        Err(GraphBackendError::TickOutsideScheduler { delay })
            if delay == SCHEDULER_BUCKETS as u32
    ));
    assert!(matches!(
        backend.import_ticks(&world, vec![tick(0, TickPriority::Highest)]),
        Err(GraphBackendError::InvalidZeroDelayPriority {
            priority: TickPriority::Highest
        })
    ));
}

#[test]
fn stale_tick_at_uncompiled_air_position_remains_until_dispatch() {
    let position = BlockPos::new(9, 20, 9);
    let stale_block = Block::StoneButton {
        button: StoneButton::new(ButtonFace::Floor, BlockDirection::North, true),
    };
    let entries = vec![TickEntry {
        pos: position,
        block_type: Some(stale_block.registry_id()),
        ticks_left: 2,
        tick_priority: TickPriority::High,
    }];
    let scheduler = entries
        .clone()
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    let mut backend = graph(&native, entries);

    assert_eq!(
        backend.scheduled_ticks(),
        native.scheduler().iter_entries().collect::<Vec<_>>()
    );
    backend.tick().unwrap();
    native.tick_interpreted();
    assert_eq!(
        backend.scheduled_ticks(),
        native.scheduler().iter_entries().collect::<Vec<_>>()
    );
    assert_eq!(backend.scheduled_ticks()[0].ticks_left, 1);

    backend.tick().unwrap();
    native.tick_interpreted();
    assert!(backend.scheduled_ticks().is_empty());
    assert!(native.scheduler().iter_entries().next().is_none());
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| line == &format!("stale {position:?} skipped"))
    );
}

#[test]
fn due_at_entry_comparator_repeats_same_value_notification() {
    let comparator_pos = NEXT_POS;
    let lamp_pos = comparator_pos.offset(BlockFace::West);
    let source_pos = comparator_pos.offset(BlockFace::East);
    let comparator = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::East, ComparatorMode::Compare, true),
    };
    let ticks = ticks_at(comparator_pos, comparator, 1, TickPriority::NanoTick);

    let mut graph_world = empty_world();
    graph_world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    graph_world.set_block(source_pos, lever(true));
    graph_world.set_block(comparator_pos, comparator);
    graph_world.set_block_entity(
        comparator_pos,
        BlockEntity::Comparator {
            output_strength: 15,
        },
    );
    graph_world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    let mut backend = graph(&graph_world, ticks.clone());

    let scheduler = ticks
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(source_pos, lever(true));
    native.set_block(comparator_pos, comparator);
    native.set_block_entity(
        comparator_pos,
        BlockEntity::Comparator {
            output_strength: 15,
        },
    );
    native.set_block(lamp_pos, Block::RedstoneLamp { lit: false });

    backend.tick().unwrap();
    native.tick_interpreted();
    backend.flush(&mut graph_world);
    assert!(
        state_at(&backend, lamp_pos).powered,
        "unchanged comparator output still delivers its callback"
    );
    assert!(matches!(
        native.get_block(lamp_pos),
        Block::RedstoneLamp { lit: true }
    ));
    let comparator_node = backend.positions[&comparator_pos];
    let route = backend.program.nodes()[comparator_node.index()]
        .output_route
        .unwrap();
    assert!(
        backend
            .trace()
            .iter()
            .any(|item| item.starts_with(&format!("route {}:", route.index())))
    );
    assert_eq!(
        backend
            .trace()
            .iter()
            .filter(|line| line.as_str() == "clock.advance")
            .count(),
        1
    );
    assert!(
        backend
            .trace()
            .iter()
            .position(|line| line.starts_with("dispatch "))
            .unwrap()
            < backend
                .trace()
                .iter()
                .position(|line| line == "clock.advance")
                .unwrap()
    );
    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert_eq!(state_at(&backend, comparator_pos).output_strength, 15);
}

#[test]
fn repeater_keeps_short_pulse_and_delayed_off_transition() {
    let lever_pos = BlockPos::new(8, 20, 5);
    let repeater_pos = BlockPos::new(7, 20, 5);
    let lamp_pos = BlockPos::new(6, 20, 5);
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            facing: BlockDirection::East,
            ..Default::default()
        },
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(lever_pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(lever_pos, lever(false));
        world.set_block(repeater_pos, repeater);
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = graph(&graph_world, vec![]);
    backend.use_block(lever_pos).unwrap();
    set_native_lever(&mut native, lever_pos, true);
    backend.tick().unwrap();
    native.tick_interpreted();
    assert!(!state_at(&backend, repeater_pos).powered);

    backend.use_block(lever_pos).unwrap();
    set_native_lever(&mut native, lever_pos, false);
    for expected in [true, true, false] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world);
        assert_eq!(state_at(&backend, repeater_pos).powered, expected);
        assert_eq!(
            graph_world.get_block(repeater_pos),
            native.get_block(repeater_pos)
        );
        assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    }
}

#[test]
fn captured_repeater_lock_change_uses_captured_powered_state() {
    let repeater_pos = NEXT_POS;
    let side_pos = repeater_pos.offset(BlockFace::South);
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            facing: BlockDirection::East,
            powered: true,
            ..Default::default()
        },
    };
    let side = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, true),
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(repeater_pos, repeater);
        world.set_block(side_pos, side);
        world.set_block_entity(
            side_pos,
            BlockEntity::Comparator {
                output_strength: 15,
            },
        );
    }
    let mut backend = graph(&graph_world, vec![]);
    let node = backend.positions[&repeater_pos];
    assert!(backend.repeater_locked(node));
    assert!(state_at(&backend, repeater_pos).powered);

    let captured = ObservedSite {
        profile: None,
        receiver: Some(CallbackReceiver::Component(node)),
        properties: ObservedProperties::Electrical {
            powered: false,
            locked: false,
        },
    };
    backend.update_repeater(node, Some(&captured)).unwrap();

    let captured_repeater = RedstoneRepeater {
        powered: false,
        locked: false,
        ..match repeater {
            Block::RedstoneRepeater { repeater } => repeater,
            _ => unreachable!(),
        }
    };
    crate::redstone::repeater::on_neighbor_updated(captured_repeater, &mut native, repeater_pos);
    let graph_state = state_at(&backend, repeater_pos);
    assert!(!graph_state.powered);
    assert_eq!(graph_state.output_strength, 0);
    assert!(graph_state.repeater_locked);
    assert_eq!(
        backend.node_block(node),
        native.get_block(repeater_pos),
        "captured powered and locked properties match native handling"
    );
}

#[test]
fn copper_bulb_uses_power_latch_separately_from_lit_state() {
    let bulb_pos = NEXT_POS;
    let mut native = empty_world();
    let mut graph_world = empty_world();
    let bulb = Block::from_name("copper_bulb").unwrap();
    for world in [&mut native, &mut graph_world] {
        world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(LEVER_POS, lever(false));
        world.set_block(bulb_pos, bulb);
    }
    let mut backend = graph(&graph_world, vec![]);
    for (powered, lit) in [(true, true), (false, true), (true, false)] {
        backend.use_block(LEVER_POS).unwrap();
        set_native_lever(&mut native, LEVER_POS, powered);
        backend.flush(&mut graph_world);
        assert_eq!(graph_world.get_block(bulb_pos), native.get_block(bulb_pos));
        assert_eq!(
            graph_world.get_block(bulb_pos).copper_bulb_state(),
            Some((lit, powered))
        );
        assert_eq!(state_at(&backend, bulb_pos).powered, powered);
        assert_eq!(
            state_at(&backend, bulb_pos).output_strength,
            if lit { 15 } else { 0 }
        );
    }
}

#[test]
fn bulb_far_comparator_callbacks_match_native_order_and_count() {
    let bulb_pos = BlockPos::new(7, 20, 5);
    let lever_pos = bulb_pos.offset(BlockFace::East);
    let support_pos = lever_pos.offset(BlockFace::Bottom);
    let spacer_pos = bulb_pos.offset(BlockFace::West);
    let comparator_pos = spacer_pos.offset(BlockFace::West);
    let comparator = Block::RedstoneComparator {
        comparator: RedstoneComparator::new(BlockDirection::East, ComparatorMode::Compare, false),
    };
    let bulb = Block::from_name("copper_bulb").unwrap();
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(support_pos, Block::Stone {});
        world.set_block(lever_pos, lever(false));
        world.set_block(bulb_pos, bulb);
        world.set_block(spacer_pos, Block::Stone {});
        world.set_block(comparator_pos, comparator);
        world.set_block_entity(
            comparator_pos,
            BlockEntity::Comparator { output_strength: 0 },
        );
    }
    let mut backend = graph(&graph_world, vec![]);
    let comparator_node = backend.positions[&comparator_pos];

    backend.use_block(lever_pos).unwrap();
    let native_trace =
        crate::redstone::instant_piston_tests::capture_at(&[bulb_pos, comparator_pos], || {
            set_native_lever(&mut native, lever_pos, true)
        });
    let native_callbacks: Vec<_> = native_trace
        .iter()
        .filter(|entry| entry["kind"] == "callback")
        .map(|entry| {
            (
                serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
            )
        })
        .collect();
    let graph_callbacks: Vec<_> = backend
        .callback_order
        .iter()
        .filter(|(position, _)| [bulb_pos, comparator_pos].contains(position))
        .map(|(position, kind)| {
            let DeliveryKind::Update { direction } = kind else {
                unreachable!()
            };
            (*position, *direction)
        })
        .collect();
    assert_eq!(graph_callbacks, native_callbacks);
    assert_eq!(
        native_callbacks
            .iter()
            .filter(|(pos, _)| *pos == comparator_pos)
            .count(),
        1
    );
    assert_eq!(
        backend
            .comparator_updates
            .iter()
            .filter(|&&position| position == comparator_pos)
            .count(),
        1
    );
    assert_eq!(
        backend.scheduled_ticks(),
        native.scheduler().iter_entries().collect::<Vec<_>>()
    );
    assert_eq!(
        backend.comparator_updates,
        [comparator_pos],
        "far comparator is notified by the compiled bulb route exactly once"
    );
    assert!(matches!(
        native.get_block(bulb_pos),
        block if block.copper_bulb_state() == Some((true, true))
    ));
    assert!(state_at(&backend, bulb_pos).powered);
    assert_eq!(state_at(&backend, comparator_pos).output_strength, 0);
    assert_eq!(backend.positions[&comparator_pos], comparator_node);
}

#[test]
fn torch_uses_compiled_conductor_query_and_scheduled_edges() {
    let torch_pos = NEXT_POS;
    let support_pos = torch_pos.offset(BlockFace::Bottom);
    let lever_pos = support_pos.offset(BlockFace::West);
    let wall_lever = Block::Lever {
        lever: Lever::new(LeverFace::Wall, BlockDirection::West, false),
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(support_pos, Block::Stone {});
        world.set_block(torch_pos, Block::RedstoneTorch { lit: true });
        world.set_block(lever_pos.offset(BlockFace::East), Block::Stone {});
        world.set_block(lever_pos, wall_lever);
    }
    let mut backend = graph(&graph_world, vec![]);

    backend.use_block(lever_pos).unwrap();
    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::East);
    for expected in [true, false] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world);
        assert_eq!(state_at(&backend, torch_pos).powered, expected);
        assert_eq!(
            graph_world.get_block(torch_pos),
            native.get_block(torch_pos)
        );
    }

    backend.use_block(lever_pos).unwrap();
    set_native_lever_with_support(&mut native, lever_pos, false, BlockFace::East);
    for expected in [false, true] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world);
        assert_eq!(state_at(&backend, torch_pos).powered, expected);
        assert_eq!(
            graph_world.get_block(torch_pos),
            native.get_block(torch_pos)
        );
    }
}

fn frozen_clipboard(name: &str) -> WorldEditClipboard {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data/instant-pistons-io");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(directory.join("download-manifest.json")).unwrap())
            .unwrap();
    let entry = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .expect("fixture belongs to frozen manifest");
    let bytes = std::fs::read(directory.join(name)).unwrap();
    assert_eq!(entry["bytes"].as_u64().unwrap() as usize, bytes.len());
    assert_eq!(entry["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    load_schematic(std::io::Cursor::new(bytes)).unwrap()
}

fn frozen_component(name: &str, matches: impl Fn(Block) -> bool) -> Block {
    let clipboard = frozen_clipboard(name);
    let mut index = 0;
    for y in 0..clipboard.size_y {
        for z in 0..clipboard.size_z {
            for x in 0..clipboard.size_x {
                let block = Block::from_id(clipboard.data.get_entry(index));
                index += 1;
                if matches(block) {
                    println!("frozen witness: {name} @ ({x},{y},{z}): {block:?}");
                    return block;
                }
            }
        }
    }
    panic!("{name} has no requested fixed component");
}

fn fixture_torch(lit: bool) -> Block {
    match frozen_component("INSTANT_TORCH.schem", |block| {
        matches!(
            block,
            Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. }
        )
    }) {
        Block::RedstoneTorch { .. } => Block::RedstoneTorch { lit },
        Block::RedstoneWallTorch { facing, .. } => Block::RedstoneWallTorch { facing, lit },
        _ => unreachable!(),
    }
}

#[test]
fn boundary_torch_power_is_not_silently_compiled_as_zero() {
    let lamp = NEXT_POS;
    let source = lamp.offset(BlockFace::North);
    let mut world = empty_world();
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(source, fixture_torch(true));
    // A wall torch excludes one horizontal face; use a face that actually emits.
    let source = if redstone::redstone_lamp_should_be_lit(&world, lamp) {
        source
    } else {
        let torch = world.get_block(source);
        world.set_block(source, Block::Air);
        let source = lamp.offset(BlockFace::South);
        world.set_block(source, torch);
        source
    };
    assert!(redstone::redstone_lamp_should_be_lit(&world, lamp));
    let full = graph(&world, vec![]);
    assert_eq!(
        full.input_strength(full.positions[&lamp], InputPort::Main),
        15
    );
    match GraphBackend::compile(&world, (lamp, lamp), vec![], Arc::default()) {
        Err(error) => assert!(
            error.to_string().contains(&format!("{source:?}")),
            "external-source rejection must identify the source: {error}"
        ),
        Ok(mut backend) => {
            let node = backend.positions[&lamp];
            assert_eq!(
                backend.input_strength(node, InputPort::Main),
                15,
                "an admitted external torch must not become zero power"
            );
            backend
                .update_component(node, DeliveryKind::Update { direction: None }, None)
                .unwrap();
            redstone::update(world.get_block(lamp), &mut world, lamp, None);
            assert_eq!(backend.node_block(node), world.get_block(lamp));
        }
    }
}

#[test]
fn imported_stale_and_inert_ticks_are_retained_or_explicitly_rejected() {
    let repeater = frozen_component("ADDER_1BIT.schem", |block| {
        matches!(block, Block::RedstoneRepeater { .. })
    });
    let removed = NEXT_POS;
    let inert = NEXT_POS.offset(BlockFace::East);
    let entries = vec![
        TickEntry {
            pos: removed,
            block_type: Some(repeater.registry_id()),
            ticks_left: 4,
            tick_priority: TickPriority::High,
        },
        TickEntry {
            pos: inert,
            block_type: Some(Block::Stone {}.registry_id()),
            ticks_left: 4,
            tick_priority: TickPriority::Normal,
        },
    ];
    let scheduler = entries
        .clone()
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(LEVER_POS, lever(false));
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(inert, Block::Stone {});
    assert_eq!(native.get_block(removed), Block::Air);
    let before = native.scheduler().iter_entries().collect::<Vec<_>>();
    assert_eq!(before, entries);
    match GraphBackend::compile(&native, BOUNDS, entries.clone(), Arc::default()) {
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("tick")
                    && entries
                        .iter()
                        .any(|entry| message.contains(&format!("{:?}", entry.pos))),
                "rejection must identify the unbound scheduled target: {message}"
            );
            assert_eq!(
                native.scheduler().iter_entries().collect::<Vec<_>>(),
                before
            );
        }
        Ok(mut backend) => {
            assert_eq!(
                backend.scheduled_ticks(),
                before,
                "stale/inert work is still pending until its dispatch boundary"
            );
            for _ in 0..4 {
                backend.tick().unwrap();
                native.tick_interpreted();
                assert_eq!(
                    backend.scheduled_ticks(),
                    native.scheduler().iter_entries().collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn removed_repeater_tick_keeps_its_deadline_until_native_stale_dispatch() {
    let repeater = frozen_component("ADDER_1BIT.schem", |block| {
        matches!(block, Block::RedstoneRepeater { .. })
    });
    let removed = NEXT_POS;
    let entry = TickEntry {
        pos: removed,
        block_type: Some(repeater.registry_id()),
        ticks_left: 4,
        tick_priority: TickPriority::High,
    };
    let scheduler = vec![entry]
        .into_iter()
        .collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(LEVER_POS, lever(false));
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    let before = native.scheduler().iter_entries().collect::<Vec<_>>();
    match GraphBackend::compile(&native, BOUNDS, before.clone(), Arc::default()) {
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("tick") && message.contains(&format!("{removed:?}")),
                "{message}"
            );
            assert_eq!(
                native.scheduler().iter_entries().collect::<Vec<_>>(),
                before
            );
        }
        Ok(mut backend) => {
            assert_eq!(backend.scheduled_ticks(), before);
            let mut dispatches = Vec::new();
            for _ in 0..4 {
                let records = capture_at(&[removed], || native.tick_interpreted());
                dispatches.push(
                    records
                        .iter()
                        .filter(|record| record["kind"] == "scheduled_tick")
                        .count(),
                );
                backend.tick().unwrap();
                assert_eq!(
                    backend.scheduled_ticks(),
                    native.scheduler().iter_entries().collect::<Vec<_>>()
                );
                assert_eq!(
                    backend.clock.logical_tick,
                    native.piston_state().logical_tick
                );
            }
            assert_eq!(dispatches, [0, 0, 0, 1]);
            assert_eq!(native.get_block(removed), Block::Air);
        }
    }
}

#[test]
fn observer_shape_then_explicit_update_matches_native_for_every_orientation() {
    let template = frozen_component("INSTANT_OBSERVER.schem", |block| {
        matches!(block, Block::Observer { .. })
    });
    let Block::Observer { observer: template } = template else {
        unreachable!()
    };
    let position = BlockPos::new(7, 20, 7);
    let mut mismatches = Vec::new();
    let original_facing = BlockFace::from(template.facing);
    let facings = std::iter::once(original_facing).chain(
        BlockFace::values()
            .into_iter()
            .filter(|face| *face != original_facing),
    );
    for facing in facings {
        for watches_changed_cell in [false, true] {
            let mut native = empty_world();
            let observer = Block::Observer {
                observer: RedstoneObserver {
                    facing: facing.into(),
                    powered: false,
                },
            };
            native.set_block(position, observer);
            let mut backend = graph(&native, vec![]);
            let node = backend.positions[&position];
            let shape_direction = if watches_changed_cell {
                facing.opposite()
            } else {
                facing
            };
            crate::interaction::change(observer, &mut native, position, shape_direction);
            backend
                .update_component(
                    node,
                    DeliveryKind::Shape {
                        direction: shape_direction,
                    },
                    None,
                )
                .unwrap();
            if backend.is_pending(node) != native.pending_tick_at(position) {
                mismatches.push((facing, watches_changed_cell, "after_shape", 0));
            }
            let update_direction = shape_direction.opposite();
            redstone::update(
                native.get_block(position),
                &mut native,
                position,
                Some(update_direction),
            );
            backend
                .update_component(
                    node,
                    DeliveryKind::Update {
                        direction: Some(update_direction),
                    },
                    None,
                )
                .unwrap();
            if backend.is_pending(node) != native.pending_tick_at(position) {
                mismatches.push((facing, watches_changed_cell, "after_update", 0));
            }
            for tick in 1..=4 {
                native.tick_interpreted();
                backend.tick().unwrap();
                if backend.node_block(node) != native.get_block(position) {
                    mismatches.push((facing, watches_changed_cell, "pulse", tick));
                }
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "only the explicit watched-face Update starts the pulse: {mismatches:?}"
    );
}

#[test]
fn torch_shape_does_not_sample_power_before_the_update_phase() {
    let position = BlockPos::new(7, 20, 7);
    let torch = fixture_torch(true);
    let mut native = empty_world();
    native.set_block(position, torch);
    let (support, _) = crate::interaction::attachment_support(torch, position).unwrap();
    native.set_block(support, Block::Stone {});
    let control_face = [
        BlockFace::North,
        BlockFace::South,
        BlockFace::East,
        BlockFace::West,
    ]
    .into_iter()
    .find(|face| support.offset(*face) != position)
    .unwrap();
    native.set_block(
        support.offset(control_face),
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, control_face.unwrap_direction(), true),
        },
    );
    let mut backend = graph(&native, vec![]);
    let node = backend.positions[&position];
    assert_eq!(backend.input_strength(node, InputPort::Main), 15);
    assert!(crate::interaction::is_valid_position(
        torch, &native, position
    ));
    crate::interaction::change(torch, &mut native, position, BlockFace::East);
    backend
        .update_component(
            node,
            DeliveryKind::Shape {
                direction: BlockFace::East,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        backend.is_pending(node),
        native.pending_tick_at(position),
        "a valid Shape callback must not enqueue an electrical recheck"
    );
    redstone::update(torch, &mut native, position, Some(BlockFace::East));
    backend
        .update_component(
            node,
            DeliveryKind::Update {
                direction: Some(BlockFace::East),
            },
            None,
        )
        .unwrap();
    assert_eq!(backend.is_pending(node), native.pending_tick_at(position));
}

#[test]
fn captured_repeater_lock_rewrite_keeps_native_output_strength_and_consumers() {
    let template = frozen_component("ADDER_1BIT.schem", |block| {
        matches!(block, Block::RedstoneRepeater { .. })
    });
    let Block::RedstoneRepeater { repeater: template } = template else {
        unreachable!()
    };
    let position = BlockPos::new(7, 20, 7);
    let mut mismatches = Vec::new();
    for captured_powered in [false, true] {
        let mut native = empty_world();
        native.set_block(
            position,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    powered: !captured_powered,
                    locked: false,
                    ..template
                },
            },
        );
        let side = template.facing.rotate();
        native.set_block(
            position.offset(side.block_face()),
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing: side,
                    powered: true,
                    locked: false,
                    ..Default::default()
                },
            },
        );
        let lamp = position.offset(template.facing.opposite().block_face());
        native.set_block(lamp, Block::RedstoneLamp { lit: false });
        let mut backend = graph(&native, vec![]);
        let node = backend.positions[&position];
        let observed = ObservedSite {
            profile: None,
            receiver: Some(CallbackReceiver::Component(node)),
            properties: ObservedProperties::Electrical {
                powered: captured_powered,
                locked: false,
            },
        };
        redstone::repeater::on_neighbor_updated(
            RedstoneRepeater {
                powered: captured_powered,
                locked: false,
                ..template
            },
            &mut native,
            position,
        );
        backend
            .update_component(
                node,
                DeliveryKind::Update {
                    direction: Some(BlockFace::Top),
                },
                Some(&observed),
            )
            .unwrap();
        let native_strength =
            redstone::source_strength(native.get_block(position), &native, position);
        redstone::update(native.get_block(lamp), &mut native, lamp, None);
        let lamp_node = backend.positions[&lamp];
        backend
            .update_component(lamp_node, DeliveryKind::Update { direction: None }, None)
            .unwrap();
        let actual = (
            backend.node_block(node),
            backend.node_state(node).output_strength,
            backend.node_block(lamp_node),
        );
        let expected = (
            native.get_block(position),
            native_strength,
            native.get_block(lamp),
        );
        if actual != expected {
            mismatches.push((captured_powered, actual, expected));
        }
    }
    assert!(
        mismatches.is_empty(),
        "captured lock rewrites must preserve the emitted strength and downstream lamp: {mismatches:?}"
    );
}

#[test]
fn bulb_far_comparator_callbacks_match_native_order_and_multiplicity() {
    let bulb = BlockPos::new(7, 20, 7);
    let control = bulb.offset(BlockFace::Top);
    let mut native = empty_world();
    native.set_block(bulb, Block::from_name("copper_bulb").unwrap());
    native.set_block(control, lever(false));
    let faces = [
        BlockFace::North,
        BlockFace::South,
        BlockFace::East,
        BlockFace::West,
    ];
    let mut comparators = Vec::new();
    for face in faces {
        let support = bulb.offset(face);
        native.set_block(support, Block::Stone {});
        let position = support.offset(face);
        native.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(
                    face.opposite().unwrap_direction(),
                    ComparatorMode::Compare,
                    false,
                ),
            },
        );
        native.set_block_entity(position, BlockEntity::Comparator { output_strength: 0 });
        comparators.push(position);
    }
    let mut backend = graph(&native, vec![]);
    let records = capture_at(&comparators, || {
        set_native_lever(&mut native, control, true)
    });
    let expected: Vec<BlockPos> = records
        .iter()
        .filter(|entry| entry["kind"] == "callback")
        .map(|entry| {
            assert!(entry["data"]["dir"].is_null());
            serde_json::from_value(entry["data"]["pos"].clone()).unwrap()
        })
        .collect();
    assert_eq!(expected, comparators);
    backend.use_block(control).unwrap();
    let actual: Vec<_> = backend
        .comparator_updates
        .iter()
        .copied()
        .filter(|position| comparators.contains(position))
        .collect();
    assert_eq!(
        actual, expected,
        "same final output does not justify additional far callbacks"
    );
    let delivered: Vec<_> = backend
        .callback_order
        .iter()
        .filter_map(|(position, kind)| comparators.contains(position).then_some((*position, *kind)))
        .collect();
    let expected_deliveries: Vec<_> = expected
        .into_iter()
        .map(|position| (position, DeliveryKind::Update { direction: None }))
        .collect();
    assert_eq!(delivered, expected_deliveries);
}

#[test]
fn isolated_empty_ordinary_and_sticky_pistons_compile() {
    for sticky in [false, true] {
        let mut world = empty_world();
        let base = BlockPos::new(5, 20, 5);
        world.set_block(
            base,
            Block::Piston {
                piston: mchprs_blocks::blocks::RedstonePiston {
                    facing: mchprs_blocks::BlockFacing::East,
                    sticky,
                    extended: false,
                },
            },
        );
        if sticky {
            world.set_block(base.offset(BlockFace::East), Block::RedstoneBlock);
        }
        assert!(
            matches!(
                GraphBackend::compile(&world, BOUNDS, vec![], Arc::default()),
                Ok(_)
            ),
            "isolated sticky={sticky} piston should be admitted"
        );
    }
}

fn isolated_piston_pair(sticky: bool) -> (PlotWorld, PlotWorld, BlockPos, BlockPos) {
    let mut native = empty_world();
    let mut graph_world = empty_world();
    let base = BlockPos::new(5, 20, 5);
    let lever_pos = base.offset(BlockFace::West);
    let piston = Block::Piston {
        piston: mchprs_blocks::blocks::RedstonePiston {
            facing: mchprs_blocks::BlockFacing::East,
            sticky,
            extended: false,
        },
    };
    for world in [&mut native, &mut graph_world] {
        world.set_block(base, piston);
        if sticky {
            world.set_block(base.offset(BlockFace::East), Block::RedstoneBlock);
        }
        world.set_block(lever_pos.offset(BlockFace::West), Block::Stone {});
        world.set_block(
            lever_pos,
            Block::Lever {
                lever: Lever::new(LeverFace::Wall, BlockDirection::East, false),
            },
        );
    }
    (native, graph_world, base, lever_pos)
}

fn piston_state_at(backend: &GraphBackend, position: BlockPos) -> PistonState {
    let (index, _) = backend
        .program
        .pistons()
        .iter()
        .enumerate()
        .find(|(_, piston)| backend.program.nodes()[piston.node.index()].position == Some(position))
        .unwrap();
    backend.state.pistons[index]
}

fn tick_native_bounded(world: &mut PlotWorld) {
    let target_tick = world.piston_state().logical_tick + 1;
    for _ in 0..128 {
        world.nanotick_advance(1);
        if world.piston_state().logical_tick == target_tick
            && world.piston_state().phase == mchprs_world::AdvancePhase::BetweenTicks
        {
            return;
        }
    }
    panic!("native piston tick did not settle within 128 operations");
}

fn piston_contact_block(backend: &GraphBackend, position: BlockPos) -> Block {
    let site = backend
        .program
        .sites()
        .iter()
        .position(|site| site.position == position && matches!(site.source, SiteSource::Contact(_)))
        .unwrap();
    backend
        .resolve_site(SiteId::new(site as u32), None)
        .unwrap()
        .block
}

#[test]
fn same_value_quasi_updates_match_native_and_deduplicate_extension() {
    let (mut native, mut graph_world, base, _) = isolated_piston_pair(false);
    let quasi_source = base.offset(BlockFace::Top).offset(BlockFace::East);
    native.set_block(quasi_source, Block::RedstoneBlock);
    graph_world.set_block(quasi_source, Block::RedstoneBlock);
    let mut backend = graph(&graph_world, vec![]);
    let piston_node = backend.program.pistons()[0].node;

    assert_eq!(backend.input_strength(piston_node, InputPort::Main), 0);
    assert!(backend.input_strength(piston_node, InputPort::Quasi) > 0);
    assert!(redstone::piston::should_piston_extend(
        &native,
        mchprs_blocks::BlockFacing::East,
        base
    ));

    for _ in 0..2 {
        redstone::update(native.get_block(base), &mut native, base, None);
        backend
            .update_component(piston_node, DeliveryKind::Update { direction: None }, None)
            .unwrap();
    }
    assert_eq!(native.piston_state().events.len(), 1);
    assert_eq!(backend.piston_requests.len(), 1);

    tick_native_bounded(&mut native);
    backend.tick().unwrap();
    assert!(matches!(native.get_block(base), Block::Piston { piston } if piston.extended));
    assert!(piston_state_at(&backend, base).extended);
}

#[test]
fn piston_acceptance_cancellation_matches_interpreter() {
    let (mut native, graph_world, base, lever_pos) = isolated_piston_pair(true);
    let mut backend = graph(&graph_world, vec![]);

    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    set_native_lever_with_support(&mut native, lever_pos, false, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    tick_native_bounded(&mut native);
    backend.tick().unwrap();

    assert!(matches!(native.get_block(base), Block::Piston { piston } if !piston.extended));
    assert!(!piston_state_at(&backend, base).extended);
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| line.contains("piston cancelled"))
    );
}

#[test]
fn piston_early_reversal_matches_interpreter_and_invalidates_old_restoration() {
    let (mut native, graph_world, base, lever_pos) = isolated_piston_pair(true);
    let mut backend = graph(&graph_world, vec![]);
    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    tick_native_bounded(&mut native);
    backend.tick().unwrap();
    assert!(matches!(native.get_block(base), Block::Piston { piston } if piston.extended));
    assert!(piston_state_at(&backend, base).extended);

    set_native_lever_with_support(&mut native, lever_pos, false, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    assert!(matches!(native.piston_state().events.back(), Some(event)
        if event.action == mchprs_world::PistonAction::RetractWithoutPull));
    native.nanotick_advance(1);
    backend.tick().unwrap();
    assert_eq!(
        piston_state_at(&backend, base).availability,
        PistonAvailability::BasePending
    );
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| line.contains("piston accept") && line.contains("RetractWithoutPull"))
    );
    assert_eq!(
        native.get_block(base.offset(BlockFace::East).offset(BlockFace::East)),
        Block::RedstoneBlock
    );
    assert_eq!(
        piston_contact_block(
            &backend,
            base.offset(BlockFace::East).offset(BlockFace::East)
        ),
        Block::RedstoneBlock
    );
    tick_native_bounded(&mut native);
    backend.tick().unwrap();
    assert_eq!(
        piston_state_at(&backend, base).availability,
        PistonAvailability::BasePending
    );
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| line.contains("piston stale"))
    );
    tick_native_bounded(&mut native);
    backend.tick().unwrap();
    assert!(matches!(native.get_block(base), Block::Piston { piston } if !piston.extended));
    assert!(!piston_state_at(&backend, base).extended);
    assert_eq!(
        piston_state_at(&backend, base).availability,
        PistonAvailability::Ready
    );
    assert_eq!(
        piston_contact_block(&backend, base.offset(BlockFace::East)),
        Block::Air
    );
    assert_eq!(
        native.get_block(base.offset(BlockFace::East).offset(BlockFace::East)),
        piston_contact_block(
            &backend,
            base.offset(BlockFace::East).offset(BlockFace::East)
        )
    );
}

#[test]
fn piston_delayed_restoration_matches_interpreter() {
    let (mut native, mut graph_world, base, lever_pos) = isolated_piston_pair(true);
    let far = base.offset(BlockFace::East).offset(BlockFace::East);
    let far_lamp = far.offset(BlockFace::North);
    for world in [&mut native, &mut graph_world] {
        world.set_block(far_lamp, Block::RedstoneLamp { lit: false });
    }
    let mut backend = graph(&graph_world, vec![]);
    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();

    for _ in 0..3 {
        tick_native_bounded(&mut native);
        backend.tick().unwrap();
        backend.flush(&mut graph_world);
        let state = piston_state_at(&backend, base);
        if native.piston_state().logical_tick < 3 {
            assert_eq!(state.availability, PistonAvailability::HeadPending);
            assert!(matches!(native.get_block(far), Block::MovingPiston { .. }));
            assert_eq!(piston_contact_block(&backend, far), Block::Air);
            assert_eq!(
                native.get_block(far_lamp),
                Block::RedstoneLamp { lit: false }
            );
            assert_eq!(
                graph_world.get_block(far_lamp),
                Block::RedstoneLamp { lit: false }
            );
        }
    }
    assert!(matches!(native.get_block(base), Block::Piston { piston } if piston.extended));
    assert!(matches!(
        native.get_block(base.offset(BlockFace::East)),
        Block::PistonHead { .. }
    ));
    assert!(piston_state_at(&backend, base).extended);
    assert_eq!(
        piston_state_at(&backend, base).availability,
        PistonAvailability::Ready
    );
    assert!(matches!(
        piston_contact_block(&backend, base.offset(BlockFace::East)),
        Block::PistonHead { .. }
    ));
    assert_eq!(
        native.get_block(base.offset(BlockFace::East).offset(BlockFace::East)),
        piston_contact_block(
            &backend,
            base.offset(BlockFace::East).offset(BlockFace::East)
        )
    );
    assert_eq!(
        native.get_block(far_lamp),
        Block::RedstoneLamp { lit: true }
    );
    assert_eq!(graph_world.get_block(far_lamp), native.get_block(far_lamp));
}

#[test]
fn ordinary_empty_piston_extension_restoration_matches_interpreter() {
    let (mut native, graph_world, base, lever_pos) = isolated_piston_pair(false);
    let far = base.offset(BlockFace::East).offset(BlockFace::East);
    let observer = Block::Observer {
        observer: RedstoneObserver {
            facing: mchprs_blocks::BlockFacing::West,
            powered: false,
        },
    };
    native.set_block(far, observer);
    let mut graph_world = graph_world;
    graph_world.set_block(far, observer);
    let mut backend = graph(&graph_world, vec![]);
    let observer_node = backend.positions[&far];
    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();

    for visit in 1..=3 {
        let graph_start = backend.callback_order.len();
        let native_trace = capture_at(&[far], || tick_native_bounded(&mut native));
        backend.tick().unwrap();
        let native_callbacks: Vec<_> = native_trace
            .iter()
            .filter(|entry| entry["kind"] == "callback")
            .map(|entry| {
                (
                    serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                    serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
                )
            })
            .collect();
        let graph_callbacks: Vec<_> = backend.callback_order[graph_start..]
            .iter()
            .filter(|(position, _)| *position == far)
            .map(|(position, kind)| {
                let DeliveryKind::Update { direction } = kind else {
                    unreachable!("observer callback should be an Update")
                };
                (*position, *direction)
            })
            .collect();
        assert_eq!(graph_callbacks, native_callbacks, "visit {visit}");
        let state = piston_state_at(&backend, base);
        assert_eq!(
            native.pending_tick_at(far),
            backend.is_pending(observer_node)
        );
        assert!(state.extended);
        if visit < 3 {
            assert_eq!(state.availability, PistonAvailability::HeadPending);
            assert!(matches!(native.get_block(base), Block::Piston { piston } if piston.extended));
        } else {
            assert_eq!(state.availability, PistonAvailability::Ready);
            assert!(matches!(
                native.get_block(base.offset(BlockFace::East)),
                Block::PistonHead { .. }
            ));
            assert!(matches!(
                piston_contact_block(&backend, base.offset(BlockFace::East)),
                Block::PistonHead { .. }
            ));
        }
    }
}

#[test]
fn external_wire_boundary_uses_regulated_sides() {
    let mut world = empty_world();
    let lamp = BlockPos::new(10, 20, 5);
    let wire = BlockPos::new(11, 20, 5);
    let source = BlockPos::new(12, 20, 5);
    world.set_block(lamp, Block::RedstoneLamp { lit: false });
    world.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
    world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(source, lever(true));
    world.set_block(source.offset(BlockFace::Bottom), Block::Stone {});

    let result = GraphBackend::compile(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(10, 31, 15)),
        vec![],
        Arc::default(),
    );
    assert!(
        matches!(
            result,
            Err(GraphBackendError::UnsupportedComponent {
                kind: "external redstone wire boundary",
                ..
            })
        ),
        "until boundary dust neighborhoods are compiled, reject the query instead of sampling stored dot sides"
    );
}

#[test]
fn moved_redstone_resource_uses_its_selected_profile_strength() {
    let mut backend = contact_resource_backend();
    let target = SiteId::new(0);
    assert_eq!(
        backend.site_power(target, PowerPort::Weak(BlockFace::West)),
        0
    );

    backend.state.resources[0].at = ContactId::new(1);

    assert_eq!(
        backend.site_power(target, PowerPort::Weak(BlockFace::West)),
        15
    );
}

#[test]
fn fixed_boundary_comparator_keeps_its_sampled_analog_strength() {
    let backend = contact_resource_backend();
    assert_eq!(backend.site_power(SiteId::new(1), PowerPort::Output), 7);
    assert_eq!(
        backend.site_power(SiteId::new(1), PowerPort::Weak(BlockFace::South)),
        7
    );
}

#[test]
fn local_power_queries_examine_only_links_for_the_requested_destination() {
    let backend = backend_with_unrelated_power_links(2_000);

    assert_eq!(backend.input_strength(NodeId::new(0), InputPort::Main), 15);
    let input_visits = backend.power_links_examined.get();

    backend.power_links_examined.set(0);
    assert_eq!(backend.conducted_power(SiteId::new(1), false), 13);
    let conductor_visits = backend.power_links_examined.get();
    assert_eq!((input_visits, conductor_visits), (1, 1));
}

#[test]
fn site_geometry_guards_use_compiled_profile_facts() {
    // Deliberately conflicting metadata tests which representation owns query facts.
    let site = SiteId::new(0);
    let (program, state) = CircuitDraft {
        nodes: vec![],
        links: vec![],
        power_links: vec![],
        comparator_inputs: vec![],
        profiles: vec![Profile {
            raw_state: Block::Stone {}.get_id(),
            registry_type: Block::Stone {}.registry_id(),
            solid: false,
            transparent: true,
            cube: false,
            electrical: ProfileElectrical::None,
        }],
        guards: vec![Guard::Site {
            site,
            view: OperandView::Live,
            test: SiteTest::Solid,
        }],
        contacts: vec![],
        routes: vec![],
        wires: vec![],
        sites: vec![SiteSpec {
            position: BlockPos::new(0, 0, 0),
            source: SiteSource::Constant(ProfileId::new(0)),
            source_strength: 0,
        }],
        pistons: vec![],
        resources: vec![],
        initial_pistons: vec![],
        initial_resources: vec![],
    }
    .lower()
    .unwrap();
    let backend = GraphBackend::new(program, state, 0).unwrap();

    assert!(!backend.guard_matches(GuardId::new(0), None));
}
