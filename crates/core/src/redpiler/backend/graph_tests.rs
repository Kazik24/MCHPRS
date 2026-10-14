//! Graph backend differential and regression tests.
use super::*;
use crate::plot::worldedit::{WorldEditClipboard, load_schematic, paste_clipboard};
use crate::plot::{PLOT_WIDTH, PlotWorld};
use crate::redpiler::backend::ScheduledBlockTick;
use crate::redstone;
use crate::redstone::instant_piston_tests::capture_at;
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::{BlockEntity, CommandBlockEntity};
use mchprs_blocks::blocks::{
    Block, ButtonFace, ComparatorMode, Instrument, Lever, LeverFace, RedstoneComparator, RedstoneObserver, RedstoneRepeater, RedstoneWire,
    RedstoneWireSide, StoneButton,
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

fn fixture_world() -> PlotWorld {
    PlotWorld::from_chunks(
        0,
        0,
        (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
        Default::default(),
    )
}

fn lever(powered: bool) -> Block {
    Block::Lever {
        lever: Lever::new(LeverFace::Floor, BlockDirection::North, powered),
    }
}

fn set_native_lever(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    set_native_lever_with_support(world, position, powered, BlockFace::Bottom);
}

fn set_native_lever_with_support(world: &mut PlotWorld, position: BlockPos, powered: bool, support: BlockFace) {
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

fn set_native_lever_as_interaction(world: &mut PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("expected lever at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    redstone::update_surrounding_blocks(world, position);
    let attachment = match lever.face {
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    redstone::update_surrounding_blocks(world, position.offset(attachment));
}

fn recording_graph(world: &PlotWorld, ticks: Vec<TickEntry>) -> GraphBackend {
    let mut backend = GraphBackend::compile(world, BOUNDS, ticks, Arc::default()).unwrap();
    backend.set_test_recording(true);
    backend
}

fn dust_path_backend(world: &PlotWorld, optimization_level: Option<usize>) -> GraphBackend {
    let monitor = TaskMonitor::default();
    let options = crate::redpiler::CompilerOptions {
        optimization_level,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        world,
        BOUNDS,
        world.scheduler().iter_entries().collect(),
        &monitor,
        crate::redpiler::analysis::AnalysisLimits::default(),
    )
    .unwrap();
    let pipeline = passes::default_graph_pipeline(&options);
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    prepared.finish_pipeline_statistics(&monitor);
    let lowered = prepared.lower(&monitor).unwrap();
    GraphBackend::from_lowered(lowered, &monitor).unwrap()
}

fn captured_wire_walk(backend: &GraphBackend, plan: &WirePlan) -> WireWalk {
    let mut walk = WireWalk::new();
    let root = backend.component_sites[plan.node.index()].unwrap();
    for site in plan.canonical_neighbors.into_iter().chain([root]) {
        let observed = backend.resolve_site(site, None).expect("wire walk site is resolvable").observed();
        walk.insert_site(
            site,
            WalkSite {
                site,
                observed,
                visited: site == root,
                x_bias: 0,
                z_bias: 0,
                layer: 0,
                heading: None,
            },
        );
    }
    walk
}

fn analog_dust_world() -> PlotWorld {
    let mut world = empty_world();
    for x in 3..=9 {
        world.set_block(BlockPos::new(x, 19, 5), Block::Stone {});
    }
    world.set_block(
        LEVER_POS,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
        },
    );
    world.set_block_entity(LEVER_POS, BlockEntity::Comparator { output_strength: 15 });
    for distance in 0..3 {
        let position = NEXT_POS + BlockPos::new(distance, 0, 0);
        world.set_block(
            position,
            Block::RedstoneWire {
                wire: RedstoneWire {
                    power: 15 - distance as u8,
                    ..Default::default()
                },
            },
        );
    }
    for distance in 0..3 {
        let position = NEXT_POS + BlockPos::new(distance, 0, 0);
        let Block::RedstoneWire { wire } = world.get_block(position) else {
            unreachable!();
        };
        world.set_block(
            position,
            Block::RedstoneWire {
                wire: redstone::wire::get_regulated_sides(wire, &world, position),
            },
        );
    }
    world
}

fn set_analog_dust_state(backend: &mut GraphBackend, input: NodeId, wires: &[NodeId], old_strength: u8, new_strength: u8) {
    let ComponentState::Electrical(input_state) = &mut backend.state.components[input.index()] else {
        panic!("analog source must have electrical state");
    };
    input_state.powered = new_strength > 0;
    input_state.output_strength = new_strength;
    for (distance, &node) in wires.iter().enumerate() {
        let ComponentState::Wire(wire) = &mut backend.state.components[node.index()] else {
            panic!("path state must remain owned by its wire node");
        };
        wire.power = old_strength.saturating_sub(distance as u8);
    }
}

#[test]
fn test_recording_can_be_toggled_around_a_graph_episode() {
    let world = empty_world();
    let mut backend = recording_graph(&world, vec![]);
    backend.set_test_recording(false);
    backend.tick().unwrap();
    assert!(backend.trace().is_empty());

    backend.set_test_recording(true);
    backend.tick().unwrap();
    assert!(!backend.trace().is_empty());
}

#[test]
fn dust_path_seed_and_captured_queries_match_for_all_strength_transitions() {
    let world = analog_dust_world();
    let mut general = dust_path_backend(&world, Some(0));
    let mut contracted = dust_path_backend(&world, None);
    let path = contracted.program.dust_paths();
    assert_eq!(path.len(), 1);
    let wires = path[0].wires.to_vec();
    let general_wires = wires.clone();
    let general_input = general.positions[&LEVER_POS];
    let contracted_input = contracted.positions[&LEVER_POS];

    for old_strength in 0..=15 {
        for new_strength in 0..=15 {
            set_analog_dust_state(&mut general, general_input, &general_wires, old_strength, new_strength);
            set_analog_dust_state(&mut contracted, contracted_input, &wires, old_strength, new_strength);
            for &node in &wires {
                let general_plan = general.wire_plan(node).clone();
                let contracted_plan = contracted.wire_plan(node).clone();
                assert_eq!(
                    contracted.seed_power(&contracted_plan),
                    general.seed_power_general(&general_plan),
                    "seed old={old_strength} new={new_strength} wire={node:?}"
                );
                let mut general_walk = captured_wire_walk(&general, &general_plan);
                let mut contracted_walk = captured_wire_walk(&contracted, &contracted_plan);
                let general_ordered = general.identify_wire_neighbors(node, &mut general_walk);
                let contracted_ordered = contracted.identify_wire_neighbors(node, &mut contracted_walk);
                assert_eq!(contracted_ordered, general_ordered);
                assert_eq!(
                    contracted.turbo_wire_power(&contracted_plan, &contracted_ordered, &contracted_walk,),
                    general.turbo_wire_power_general(&general_plan, &general_ordered, &general_walk,),
                    "captured old={old_strength} new={new_strength} wire={node:?}"
                );
            }
        }
    }

    set_analog_dust_state(&mut general, general_input, &general_wires, 7, 12);
    set_analog_dust_state(&mut contracted, contracted_input, &wires, 7, 12);
    let mut captured_general = Vec::new();
    let mut captured_contracted = Vec::new();
    for &node in &wires {
        let general_plan = general.wire_plan(node).clone();
        let contracted_plan = contracted.wire_plan(node).clone();
        let mut general_walk = captured_wire_walk(&general, &general_plan);
        let mut contracted_walk = captured_wire_walk(&contracted, &contracted_plan);
        let general_ordered = general.identify_wire_neighbors(node, &mut general_walk);
        let contracted_ordered = contracted.identify_wire_neighbors(node, &mut contracted_walk);
        captured_general.push((general_plan, general_walk, general_ordered));
        captured_contracted.push((contracted_plan, contracted_walk, contracted_ordered));
    }
    set_analog_dust_state(&mut general, general_input, &general_wires, 1, 4);
    set_analog_dust_state(&mut contracted, contracted_input, &wires, 1, 4);
    for (index, node) in wires.iter().copied().enumerate() {
        let (general_plan, general_walk, general_ordered) = &captured_general[index];
        let (contracted_plan, contracted_walk, contracted_ordered) = &captured_contracted[index];
        assert_eq!(
            contracted.turbo_wire_power(contracted_plan, contracted_ordered, contracted_walk),
            general.turbo_wire_power_general(general_plan, general_ordered, general_walk),
            "captured-old/live-new wire={node:?}"
        );
    }
    assert!(contracted.counts.dust_path_power_samples_elided > 0);
}

#[test]
fn dust_path_keeps_nested_input_callback_order_and_intermediate_writes() {
    let world = analog_dust_world();
    let mut general = dust_path_backend(&world, Some(0));
    let mut contracted = dust_path_backend(&world, None);
    let input = general.positions[&LEVER_POS];
    let wires = contracted.program.dust_paths()[0].wires.to_vec();
    general.set_test_recording(true);
    contracted.set_test_recording(true);

    for strength in [8, 3, 15, 0, 0, 7] {
        let start = general.callback_order.len();
        general.set_node(input, false, strength, None, true).unwrap();
        contracted.set_node(input, false, strength, None, true).unwrap();
        assert_eq!(
            &contracted.callback_order[start..],
            &general.callback_order[start..],
            "callback order at analog strength {strength}"
        );
        assert_eq!(contracted.trace(), general.trace());
        for &node in &wires {
            assert_eq!(
                contracted.state.components[node.index()],
                general.state.components[node.index()],
                "intermediate wire state at analog strength {strength}"
            );
        }
    }
}

fn state_at(backend: &GraphBackend, position: BlockPos) -> NodeState {
    backend.node_state(backend.positions[&position]).clone()
}

fn ticks_at(position: BlockPos, block: Block, count: usize, priority: TickPriority) -> Vec<TickEntry> {
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
        Guard::State(StateTest::ResourceAt { resource, contact: first }),
        Guard::State(StateTest::ResourceEnabled(resource)),
        Guard::All(vec![first_at, enabled].into_boxed_slice()),
        Guard::State(StateTest::ResourceAt { resource, contact: second }),
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
                    comparator: RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, true),
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
        turbo_callback_indices: vec![],
        wires: vec![],
        dust_paths: vec![],
        wire_relations: vec![],
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
            to: PowerTarget::Input { node, port: InputPort::Main },
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
            to: PowerTarget::Input { node, port: InputPort::Side },
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
        turbo_callback_indices: vec![],
        wires: vec![],
        dust_paths: vec![],
        wire_relations: vec![],
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
    let mut backend = recording_graph(&graph_world, vec![]);
    let lever_node = backend.positions[&LEVER_POS];
    let lamp_node = backend.positions[&lamp_pos];

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    assert!(state_at(&backend, lamp_pos).powered);
    assert!(matches!(native.get_block(lamp_pos), Block::RedstoneLamp { lit: true }));

    let trace = backend.trace();
    let source_change = trace.iter().position(|item| item.starts_with(&format!("state {lever_node:?}"))).unwrap();
    let lamp_change = trace.iter().position(|item| item.starts_with(&format!("state {lamp_node:?}"))).unwrap();
    let delivery = trace.iter().position(|item| item.starts_with("route ")).unwrap();
    assert!(source_change < delivery && delivery < lamp_change, "{trace:#?}");

    let state = backend.state.clone();
    let scheduled = backend.scheduled_ticks();
    let pending = backend.pending.clone();
    let clock = backend.clock;
    backend.flush(&mut graph_world, false);
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
        world.set_block(wire_pos, Block::RedstoneWire { wire: Default::default() });
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = recording_graph(&graph_world, vec![]);
    assert!(matches!(backend.node_block(backend.positions[&wire_pos]), Block::RedstoneWire { wire }
        if wire.north == RedstoneWireSide::None && wire.south == RedstoneWireSide::None
            && wire.east == RedstoneWireSide::None && wire.west == RedstoneWireSide::None));

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    backend.flush(&mut graph_world, false);

    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert!(matches!(graph_world.get_block(lamp_pos), Block::RedstoneLamp { lit: true }));
    assert!(matches!(graph_world.get_block(wire_pos), Block::RedstoneWire { wire }
        if wire.power == 15 && wire.north == RedstoneWireSide::None
            && wire.south == RedstoneWireSide::None && wire.east == RedstoneWireSide::None
            && wire.west == RedstoneWireSide::None));
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
    backend.flush(&mut graph_world, false);
    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert!(matches!(graph_world.get_block(lamp_pos), Block::RedstoneLamp { lit: true }));
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
        world.set_block(wire_pos, Block::RedstoneWire { wire: Default::default() });
        world.set_block(conductor_pos, Block::Stone {});
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = recording_graph(&graph_world, vec![]);

    backend.use_block(LEVER_POS).unwrap();
    set_native_lever(&mut native, LEVER_POS, true);
    backend.flush(&mut graph_world, false);

    assert_eq!(graph_world.get_block(lamp_pos), native.get_block(lamp_pos));
    assert!(matches!(graph_world.get_block(lamp_pos), Block::RedstoneLamp { lit: true }));
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
            world.set_block(*position, Block::RedstoneWire { wire: Default::default() });
        }
        world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    }
    let mut backend = recording_graph(&graph_world, vec![]);

    for powered in [true, false] {
        let graph_start = backend.callback_order.len();
        backend.use_block(lever_pos).unwrap();
        let native_trace = capture_at(&observed, || set_native_lever(&mut native, lever_pos, powered));
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
                let DeliveryKind::Update { direction } = kind else { unreachable!() };
                (*position, *direction)
            })
            .collect();

        backend.flush(&mut graph_world, false);
        assert_eq!(graph_callbacks, native_callbacks, "graph trace: {:#?}", backend.trace());
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
                backend.trace().iter().any(|entry| entry.starts_with(prefix)),
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
    let mut backend = recording_graph(&graph_world, vec![]);
    let node = backend.positions[&wire_pos];
    backend.update_component(node, DeliveryKind::Update { direction: None }, None).unwrap();
    redstone::update(native.get_block(wire_pos), &mut native, wire_pos, None);

    assert_eq!(backend.wire_state(node), wire);
    assert_eq!(graph_world.get_block(wire_pos), native.get_block(wire_pos));
    assert!(backend.trace().iter().any(|entry| entry.starts_with("wire seed ")));
    assert!(!backend.trace().iter().any(|entry| entry.starts_with("wire turbo start ")));
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
    let mut backend = recording_graph(&world, vec![]);

    backend.flush(&mut world, false);

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
        world.set_block(wire_pos, Block::RedstoneWire { wire: Default::default() });
        world.set_block(source_pos, Block::RedstoneBlock {});
    }
    let mut backend = recording_graph(&graph_world, vec![]);
    let node = backend.positions[&wire_pos];
    let native_trace = capture_at(&[wire_pos], || {
        crate::interaction::change(native.get_block(wire_pos), &mut native, wire_pos, BlockFace::West);
    });
    backend
        .update_component(node, DeliveryKind::Shape { direction: BlockFace::West }, None)
        .unwrap();
    backend.flush(&mut graph_world, false);

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
            world.set_block(position, Block::RedstoneWire { wire: Default::default() });
        }
    }
    let mut backend = recording_graph(&graph_world, vec![]);
    backend.use_block(lever_pos).unwrap();
    set_native_lever(&mut native, lever_pos, true);
    backend.flush(&mut graph_world, false);

    for position in wires {
        assert_eq!(graph_world.get_block(position), native.get_block(position), "wire at {position:?}");
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
    for direction in [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West] {
        let first_wire = BlockPos::new(8, 20, 8);
        let wires: Vec<_> = std::iter::successors(Some(first_wire), |position| Some(position.offset(direction)))
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
                world.set_block(*position, Block::RedstoneWire { wire: Default::default() });
            }
        }
        let mut backend = recording_graph(&graph_world, vec![]);
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
                let DeliveryKind::Update { direction } = kind else { unreachable!() };
                (*position, *direction)
            })
            .collect();

        assert_eq!(
            graph_callbacks,
            native_callbacks,
            "direction={direction:?}; graph trace: {:#?}",
            backend.trace()
        );
        backend.flush(&mut graph_world, false);
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
        world.set_block(wire_pos, Block::RedstoneWire { wire: Default::default() });
    }
    let mut backend = recording_graph(&graph_world, vec![]);

    for powered in [true, false] {
        backend.use_block(lever_pos).unwrap();
        set_native_lever(&mut native, lever_pos, powered);
        backend.flush(&mut graph_world, false);
        assert_eq!(
            graph_world.get_block(wire_pos),
            native.get_block(wire_pos),
            "powered={powered}; graph trace: {:#?}",
            backend.trace()
        );
    }
    assert!(
        backend
            .trace()
            .iter()
            .any(|entry| { entry.starts_with("wire seed ") && entry.contains("operands=live-sites/live-strong") && entry.contains("block=15") })
    );
}

#[test]
fn compiled_wire_side_sites_match_native_vertical_and_horizontal_rules() {
    let wire_pos = BlockPos::new(8, 20, 8);
    let north = wire_pos.offset(BlockFace::North);
    let east = wire_pos.offset(BlockFace::East);
    let west = wire_pos.offset(BlockFace::West);
    let mut world = empty_world();
    world.set_block(north, Block::Stone {});
    world.set_block(north.offset(BlockFace::Top), Block::RedstoneWire { wire: Default::default() });
    world.set_block(east.offset(BlockFace::Bottom), Block::RedstoneWire { wire: Default::default() });
    world.set_block(west, Block::RedstoneBlock {});

    let sites = crate::world::wire_cache::positions(wire_pos).map(|position| world.get_block(position));
    let wire = RedstoneWire::default();
    let compiled = crate::redstone::wire::get_raw_sides_from_neighbors(wire, &sites);
    let native = crate::redstone::wire::get_raw_sides_from(wire, wire_pos, |position| world.get_block(position));

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
    let observed = [lever_pos, outer_wire, bulb_pos, inner_wire, inner_source, continuation];
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
        world.set_block(outer_wire, Block::RedstoneWire { wire: Default::default() });
        world.set_block(inner_wire, Block::RedstoneWire { wire: Default::default() });
        world.set_block(continuation, Block::RedstoneWire { wire: Default::default() });
        world.set_block(bulb_pos, bulb);
        world.set_block(inner_source, lever(false));
        world.set_block(bulb_source, Block::RedstoneBlock);
    }
    let mut backend = recording_graph(&graph_world, vec![]);
    let inner_source_node = backend.positions[&inner_source];
    let outer_node = backend.positions[&outer_wire];
    let inner_node = backend.positions[&inner_wire];
    let continuation_node = backend.positions[&continuation];

    backend.set_node(inner_source_node, true, 15, None, false).unwrap();
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
            let DeliveryKind::Update { direction } = kind else { unreachable!() };
            (*position, *direction)
        })
        .collect();
    assert_eq!(graph_callbacks, native_callbacks, "{:#?}", backend.trace());

    let bulb_callback_index = backend.callback_order.iter().position(|(position, _)| *position == bulb_pos).unwrap();
    let inner_callback_index = backend
        .callback_order
        .iter()
        .enumerate()
        .skip(bulb_callback_index + 1)
        .find_map(|(index, (position, _))| (*position == inner_wire).then_some(index))
        .unwrap();
    assert!(bulb_callback_index < inner_callback_index, "{:#?}", backend.callback_order);

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
            entry["kind"] == "wire_strength" && serde_json::from_value::<BlockPos>(entry["data"]["pos"].clone()).unwrap() == continuation
        })
        .unwrap_or_else(|| panic!("nested walk did not write the continuation wire: {native_trace:#?}"));
    assert!(native_nested_callback < native_nested_strength, "{native_trace:#?}");
    assert_eq!(native_trace[native_nested_strength]["data"]["old"], 0);
    assert_eq!(native_trace[native_nested_strength]["data"]["new"], 14);

    let starts: Vec<_> = backend
        .trace()
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.starts_with("wire turbo start ").then_some((index, line)))
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
    assert!(
        backend
            .trace()
            .iter()
            .any(|line| { line.starts_with(&format!("wire strength {continuation_node:?}")) && line.ends_with("0 -> 14") })
    );

    backend.flush(&mut graph_world, false);
    for position in [outer_wire, inner_wire, continuation, bulb_pos] {
        assert_eq!(graph_world.get_block(position), native.get_block(position));
    }
    assert!(matches!(graph_world.get_block(outer_wire), Block::RedstoneWire { wire } if wire.power == 15));
    assert!(matches!(graph_world.get_block(inner_wire), Block::RedstoneWire { wire } if wire.power == 15));
    assert!(matches!(graph_world.get_block(continuation), Block::RedstoneWire { wire } if wire.power == 14));
    assert_eq!(graph_world.get_block(bulb_pos).copper_bulb_state(), Some((true, true)));
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
    let mut backend = recording_graph(&graph_world, vec![]);
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
    assert!(!backend.is_pending(observer), "wrong watched face must be ignored");

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
    assert_eq!(backend.scheduled_ticks().len(), 1, "a duplicate callback is suppressed");

    for (tick, expected_powered) in [(1, false), (2, true), (3, true), (4, false)] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world, false);
        assert_eq!(state_at(&backend, observer_pos).powered, expected_powered, "tick {tick}");
        assert_eq!(graph_world.get_block(observer_pos), native.get_block(observer_pos), "tick {tick}");
        assert_eq!(backend.clock.logical_tick, native.piston_state().logical_tick);
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
    let mut recovery = recording_graph(&recovery_world, vec![]);
    let recovery_node = recovery.positions[&observer_pos];
    recovery
        .update_component(recovery_node, DeliveryKind::Update { direction: None }, None)
        .unwrap();
    assert!(recovery.is_pending(recovery_node), "powered/None recovery schedules the off edge");
}

#[test]
fn shape_delivery_does_not_sample_observers() {
    let source = NEXT_POS;
    let away = source.offset(BlockFace::West);
    let toward = source.offset(BlockFace::East);
    let observer = |facing| Block::Observer {
        observer: RedstoneObserver { facing, powered: false },
    };
    let mut native = empty_world();
    let mut graph_world = empty_world();
    for world in [&mut native, &mut graph_world] {
        world.set_block(source, Block::Stone {});
        world.set_block(away, observer(BlockFacing::West));
        world.set_block(toward, observer(BlockFacing::West));
    }
    let mut backend = recording_graph(&graph_world, vec![]);

    for (position, shape_direction, update_direction) in [(away, BlockFace::West, BlockFace::East), (toward, BlockFace::East, BlockFace::West)] {
        let node = backend.positions[&position];
        crate::interaction::change(native.get_block(position), &mut native, position, shape_direction);
        backend
            .update_component(node, DeliveryKind::Shape { direction: shape_direction }, None)
            .unwrap();
        assert!(!backend.is_pending(node));
        assert!(!native.pending_tick_at(position));

        redstone::update(native.get_block(position), &mut native, position, Some(update_direction));
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
    let scheduler = entries.clone().into_iter().collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(first, button);
    native.set_block(second, button);
    let mut backend = recording_graph(&native, entries);
    assert_eq!(backend.scheduled_ticks().len(), 5, "duplicates and stale typed work are imported");

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
            backend
                .positions
                .iter()
                .find_map(|(position, node)| (line == &format!("dispatch {node:?}")).then_some(*position))
        })
        .collect();
    assert_eq!(dispatches, [second, first, second, first]);
    assert!(backend.trace().iter().any(|line| line.starts_with("stale ")));
    assert_eq!(backend.trace().iter().filter(|line| line.as_str() == "clock.advance").count(), 2);
}

#[test]
fn imported_wire_tick_is_consumed_as_a_native_noop() {
    let position = BlockPos::new(8, 20, 5);
    let block = Block::RedstoneWire { wire: Default::default() };
    let entry = TickEntry {
        pos: position,
        block_type: Some(block.registry_id()),
        ticks_left: 2,
        tick_priority: TickPriority::High,
    };
    let scheduler = vec![entry.clone()].into_iter().collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(position, block);
    let mut backend = recording_graph(&native, vec![entry.clone()]);
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
    assert!(backend.trace().iter().any(|line| line == &format!("dispatch {node:?}")));
}

#[test]
fn imported_tick_validation_rejects_out_of_ring_and_invalid_zero_delay() {
    let mut world = empty_world();
    world.set_block(LEVER_POS, lever(false));
    let mut backend = recording_graph(&world, vec![]);
    let target = backend.positions[&LEVER_POS];
    let tick = |delay, priority| ImportedTick {
        target: Some(target),
        position: LEVER_POS,
        block_type: Some(lever(false).registry_id()),
        delay,
        priority,
        current_registry_type: Some(lever(false).registry_id()),
    };

    assert!(matches!(
        backend.import_ticks(vec![tick(
            SCHEDULER_BUCKETS as u32,
            TickPriority::Normal
        )]),
        Err(GraphBackendError::TickOutsideScheduler { delay })
            if delay == SCHEDULER_BUCKETS as u32
    ));
    assert!(matches!(
        backend.import_ticks(vec![tick(0, TickPriority::Highest)]),
        Err(GraphBackendError::InvalidZeroDelayPriority {
            priority: TickPriority::Highest
        })
    ));
}

#[test]
fn imported_ticks_preserve_stale_air_duplicates_and_reset_deadlines() {
    let position = LEVER_POS;
    let stale = TickEntry {
        pos: position,
        block_type: Some(Block::Stone {}.registry_id()),
        ticks_left: 3,
        tick_priority: TickPriority::High,
    };
    let air_work = TickEntry {
        pos: position.offset(BlockFace::East),
        block_type: None,
        ticks_left: 5,
        tick_priority: TickPriority::Normal,
    };
    let expected = [stale.clone(), stale.clone(), air_work.clone()];
    let world = empty_world();
    let mut backend = GraphBackend::compile(
        &world,
        (position, position.offset(BlockFace::East)),
        vec![stale.clone(), stale.clone(), air_work.clone()],
        Arc::default(),
    )
    .unwrap();
    assert_eq!(backend.scheduled_ticks(), expected);

    let mut reset_world = empty_world();
    backend.reset(&mut reset_world);
    assert_eq!(reset_world.scheduler().iter_entries().collect::<Vec<_>>(), expected);
}

#[test]
fn untyped_imported_work_does_not_suppress_a_typed_callback_tick() {
    let position = NEXT_POS;
    let block = Block::Observer {
        observer: RedstoneObserver {
            facing: BlockFacing::West,
            powered: false,
        },
    };
    let untyped = TickEntry {
        pos: position,
        block_type: None,
        ticks_left: 2,
        tick_priority: TickPriority::High,
    };
    let mut world = empty_world();
    world.set_block(position, block);
    let mut backend = GraphBackend::compile(&world, BOUNDS, vec![untyped.clone()], Arc::default()).unwrap();
    let node = backend.positions[&position];
    assert!(!backend.is_pending(node));

    backend
        .update_component(
            node,
            DeliveryKind::Update {
                direction: Some(BlockFace::West),
            },
            None,
        )
        .unwrap();
    assert_eq!(backend.scheduled_ticks()[0], untyped);
    assert_eq!(backend.scheduled_ticks()[1].block_type, Some(block.registry_id()));
    assert_eq!(backend.scheduled_ticks()[1].ticks_left, 2);
    assert_eq!(backend.scheduled_ticks()[1].tick_priority, TickPriority::Normal);
    assert_eq!(backend.scheduled_ticks().len(), 2);
}

#[test]
fn imported_ticks_reject_unloaded_targets_instead_of_treating_them_as_air() {
    let position = BlockPos::new(16, 20, 5);
    let world = empty_world();
    let mut backend = recording_graph(&world, vec![]);
    let result = backend.import_ticks(vec![ImportedTick {
        target: None,
        position,
        block_type: None,
        delay: 1,
        priority: TickPriority::Normal,
        current_registry_type: None,
    }]);
    match result {
        Err(GraphBackendError::UnknownTickTarget { position: found }) if found == position => {}
        Err(error) => panic!("expected an unknown imported tick target, got {error}"),
        Ok(_) => panic!("an unloaded imported tick target was admitted"),
    }
}

#[test]
fn graph_noteblock_samples_live_instrument_and_buffers_only_eligible_edges() {
    let note_pos = BlockPos::new(6, 20, 5);
    let above = note_pos.offset(BlockFace::Top);
    let below = note_pos.offset(BlockFace::Bottom);
    let mut world = empty_world();
    world.set_block(LEVER_POS, lever(false));
    world.set_block(
        note_pos,
        Block::NoteBlock {
            instrument: Instrument::Harp,
            note: 12,
            powered: false,
        },
    );
    world.set_block(below, Block::GoldBlock);
    let mut native = empty_world();
    native.set_block(LEVER_POS, lever(false));
    native.set_block(
        note_pos,
        Block::NoteBlock {
            instrument: Instrument::Harp,
            note: 12,
            powered: false,
        },
    );
    native.set_block(below, Block::GoldBlock);
    let mut backend = recording_graph(&world, vec![]);

    backend.use_block(LEVER_POS).unwrap();
    assert!(state_at(&backend, note_pos).powered);
    assert!(matches!(
        backend.effects.as_slice(),
        [PresentationEffect::Sound {
            id,
            category: 2,
            volume: 3.0,
            pitch: 1.0,
            ..
        }] if *id == Instrument::Bell.to_sound_id()
    ));
    backend.flush(&mut world, true);
    assert!(matches!(
        world.get_block(note_pos),
        Block::NoteBlock {
            instrument: Instrument::Bell,
            note: 12,
            powered: true,
        }
    ));
    set_native_lever_as_interaction(&mut native, LEVER_POS, true);
    assert_eq!(world.get_block(note_pos), native.get_block(note_pos));
    assert!(backend.effects.is_empty());

    backend.use_block(LEVER_POS).unwrap();
    assert!(!state_at(&backend, note_pos).powered);
    assert!(backend.effects.is_empty(), "falling power does not play a note");
    backend.flush(&mut world, true);
    set_native_lever_as_interaction(&mut native, LEVER_POS, false);
    assert_eq!(world.get_block(note_pos), native.get_block(note_pos));

    world.set_block(above, Block::Stone {});
    native.set_block(above, Block::Stone {});
    let mut blocked = recording_graph(&world, vec![]);
    blocked.use_block(LEVER_POS).unwrap();
    set_native_lever_as_interaction(&mut native, LEVER_POS, true);
    assert!(state_at(&blocked, note_pos).powered);
    assert_eq!(blocked.node_block(blocked.positions[&note_pos]), native.get_block(note_pos));
    assert!(blocked.effects.is_empty(), "an occupied above block blocks sound");
}

#[test]
fn imported_note_ticks_are_noops_and_reset_keeps_future_work() {
    let position = BlockPos::new(6, 20, 5);
    let note = Block::NoteBlock {
        instrument: Instrument::Harp,
        note: 12,
        powered: false,
    };
    let entry = TickEntry {
        pos: position,
        block_type: Some(note.registry_id()),
        ticks_left: 1,
        tick_priority: TickPriority::Normal,
    };
    let mut world = empty_world();
    world.set_block(position, note);
    let mut native = PlotWorld::from_chunks(
        0,
        0,
        vec![Chunk::empty(0, 0)],
        vec![entry.clone()].into_iter().collect::<TickScheduler<ScheduledBlockTick>>(),
    );
    native.set_block(position, note);

    let mut backend = GraphBackend::compile(&world, BOUNDS, vec![entry], Arc::default()).unwrap();
    backend.tick().unwrap();
    native.tick_interpreted();
    assert!(backend.scheduled_ticks().is_empty());
    assert_eq!(backend.node_block(backend.positions[&position]), note);
    assert_eq!(native.get_block(position), note);

    let future = TickEntry {
        pos: position,
        block_type: Some(note.registry_id()),
        ticks_left: 3,
        tick_priority: TickPriority::High,
    };
    let mut backend = GraphBackend::compile(&world, BOUNDS, vec![future.clone()], Arc::default()).unwrap();
    backend.reset(&mut world);
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), [future]);
}

#[test]
fn graph_command_results_are_synchronous_and_hostless_ticks_preserve_work() {
    let command_pos = BlockPos::new(8, 20, 5);
    let lever_pos = command_pos.offset(BlockFace::North);
    let comparator_pos = BlockPos::new(7, 20, 5);
    let command_world = || {
        let mut world = empty_world();
        world.set_block(lever_pos, lever(false));
        let mut command = Block::from_name("command_block").unwrap();
        command.set_properties(std::collections::HashMap::from([("facing", "east")]));
        world.set_block(command_pos, command);
        world.set_block_entity(
            command_pos,
            BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                command: "say graph".into(),
                ..Default::default()
            })),
        );
        world.set_block(
            comparator_pos,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::East, ComparatorMode::Compare, false),
            },
        );
        world
    };
    let mut world = command_world();
    let mut native_world = command_world();
    set_native_lever_as_interaction(&mut native_world, lever_pos, true);

    let mut backend = recording_graph(&world, vec![]);
    backend.use_block(lever_pos).unwrap();
    backend.flush(&mut world, true);
    let due_before = backend.scheduler.iter().count();
    let clock_before = backend.clock.logical_tick;
    assert_eq!(backend.tick(), Err(GraphExecutionError::CommandHostUnavailable { position: command_pos }));
    assert_eq!(backend.scheduler.iter().count(), due_before);
    assert_eq!(backend.clock.logical_tick, clock_before);
    assert!(matches!(
        world.get_block_entity(command_pos),
        Some(BlockEntity::CommandBlock(entity)) if entity.powered
    ));

    backend.reset(&mut world);
    let continuation = world.scheduler().iter_entries().collect::<Vec<_>>();
    assert_eq!(continuation.len(), 1);
    assert_eq!(continuation[0].pos, command_pos);
    assert_eq!(continuation[0].ticks_left, 1);
    assert_eq!(continuation[0].tick_priority, TickPriority::Normal);
    assert_eq!(continuation, native_world.scheduler().iter_entries().collect::<Vec<_>>());
    world.clear_scheduled_ticks();
    let mut backend = recording_graph(&world, continuation);

    backend.tick_with_world(&mut world).unwrap();
    native_world.tick_interpreted();
    let ComponentState::Electrical(command_state) = &backend.state.components[backend.positions[&command_pos].index()] else {
        panic!("command block retains electrical state");
    };
    assert_eq!(command_state.output_strength, 1);
    assert_eq!(world.command_output().collect::<Vec<_>>(), [r#"{"text":"[@] graph"}"#]);
    assert!(matches!(
        world.get_block_entity(command_pos),
        Some(BlockEntity::CommandBlock(entity)) if entity.success_count == 1
    ));
    let comparator = backend.positions[&comparator_pos];
    assert_eq!(backend.comparator_output(comparator), 1);
    assert!(backend.is_pending(comparator));
    assert!(backend.comparator_updates.contains(&comparator_pos));
    let trace = backend.trace();
    let command_dispatch = trace
        .iter()
        .position(|entry| entry == &format!("dispatch {:?}", backend.positions[&command_pos]))
        .unwrap();
    let comparator_schedule = trace
        .iter()
        .position(|entry| entry == &format!("schedule {comparator:?} +1 ticks Normal"))
        .unwrap();
    assert!(command_dispatch < comparator_schedule);
    assert!(
        trace[command_dispatch..comparator_schedule]
            .iter()
            .any(|entry| entry.starts_with("route "))
    );
    assert_eq!(backend.scheduled_ticks(), native_world.scheduler().iter_entries().collect::<Vec<_>>());
    assert_eq!(backend.comparator_output(comparator), 1);
    assert!(backend.is_pending(comparator));
    assert!(native_world.pending_tick_at(comparator_pos));
    backend.tick_with_world(&mut world).unwrap();
    native_world.tick_interpreted();
    let native_comparator_output = match native_world.get_block_entity(comparator_pos) {
        Some(BlockEntity::Comparator { output_strength }) => *output_strength,
        _ => 0,
    };
    assert_eq!(backend.node_state(comparator).output_strength, native_comparator_output);
    assert_eq!(
        world.command_output().collect::<Vec<_>>(),
        native_world.command_output().collect::<Vec<_>>()
    );
    let command_entity = |world: &PlotWorld| match world.get_block_entity(command_pos) {
        Some(BlockEntity::CommandBlock(entity)) => entity.as_ref().clone(),
        _ => panic!("command block execution publishes its entity state"),
    };
    assert_eq!(command_entity(&world), command_entity(&native_world));
}

#[test]
fn graph_conditional_chain_reads_the_synchronous_predecessor_result() {
    let first = BlockPos::new(8, 20, 5);
    let second = first.offset(BlockFace::East);
    let lever_pos = first.offset(BlockFace::North);
    let mut world = empty_world();
    world.set_block(lever_pos, lever(false));
    for (position, name, command, conditional) in [
        (first, "command_block", "stop", false),
        (second, "chain_command_block", "say should_not_run", true),
    ] {
        let mut block = Block::from_name(name).unwrap();
        block.set_properties(std::collections::HashMap::from([
            ("facing", "east"),
            ("conditional", if conditional { "true" } else { "false" }),
        ]));
        world.set_block(position, block);
        world.set_block_entity(
            position,
            BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                command: command.into(),
                automatic: conditional,
                ..Default::default()
            })),
        );
    }
    let mut backend = recording_graph(&world, vec![]);
    backend.use_block(lever_pos).unwrap();
    backend.tick_with_world(&mut world).unwrap();
    let ComponentState::Electrical(first_state) = &backend.state.components[backend.positions[&first].index()] else {
        panic!("command block retains electrical state");
    };
    let ComponentState::Electrical(second_state) = &backend.state.components[backend.positions[&second].index()] else {
        panic!("chain command block retains electrical state");
    };
    assert_eq!(first_state.output_strength, 0);
    assert_eq!(second_state.output_strength, 0);
    assert_eq!(world.command_output().count(), 0);
}

#[test]
fn graph_repeating_automatic_command_rearms_with_one_future_tick() {
    let position = BlockPos::new(8, 20, 5);
    let mut world = empty_world();
    let block = Block::from_name("repeating_command_block").unwrap();
    world.set_block(position, block);
    world.set_block_entity(
        position,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say repeat".into(),
            automatic: true,
            ..Default::default()
        })),
    );
    let mut backend = recording_graph(&world, vec![]);
    assert_eq!(backend.scheduled_ticks().len(), 1);
    assert_eq!(backend.scheduled_ticks()[0].ticks_left, 1);

    backend.tick_with_world(&mut world).unwrap();
    assert_eq!(world.command_output().count(), 1);
    assert_eq!(backend.scheduled_ticks().len(), 1);
    assert_eq!(backend.scheduled_ticks()[0].pos, position);
    assert_eq!(backend.scheduled_ticks()[0].ticks_left, 1);

    backend.tick_with_world(&mut world).unwrap();
    assert_eq!(world.command_output().count(), 2);
    assert!(matches!(
        world.get_block_entity(position),
        Some(BlockEntity::CommandBlock(entity))
            if entity.success_count == 1 && entity.last_execution == 2
    ));
}

#[test]
fn imported_automatic_command_keeps_its_captured_condition() {
    let previous = BlockPos::new(8, 20, 5);
    let command_pos = previous.offset(BlockFace::East);
    let command = |conditional| {
        let mut block = Block::from_name("command_block").unwrap();
        block.set_properties(std::collections::HashMap::from([
            ("facing", "east"),
            ("conditional", if conditional { "true" } else { "false" }),
        ]));
        block
    };
    let previous_block = command(false);
    let conditional_block = command(true);
    let imported = TickEntry {
        pos: command_pos,
        block_type: Some(conditional_block.registry_id()),
        ticks_left: 1,
        tick_priority: TickPriority::Normal,
    };
    let make_world = || {
        let mut world = empty_world();
        world.set_block(previous, previous_block);
        world.set_block(command_pos, conditional_block);
        world.set_block_entity(
            previous,
            BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                success_count: 1,
                ..Default::default()
            })),
        );
        world.set_block_entity(
            command_pos,
            BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                command: "say captured false".into(),
                automatic: true,
                condition_met: false,
                ..Default::default()
            })),
        );
        world
    };
    let mut world = make_world();
    let mut native = make_world();
    native.restore_scheduled_tick(imported.clone());
    let mut backend = GraphBackend::compile(&world, BOUNDS, vec![imported], Arc::default()).unwrap();

    backend.tick_with_world(&mut world).unwrap();
    native.tick_interpreted();

    assert_eq!(world.command_output().count(), 0);
    assert_eq!(native.command_output().count(), 0);
    assert!(matches!(
        world.get_block_entity(command_pos),
        Some(BlockEntity::CommandBlock(entity))
            if !entity.condition_met && entity.success_count == 0
    ));
    let command_entity = |world: &PlotWorld| match world.get_block_entity(command_pos) {
        Some(BlockEntity::CommandBlock(entity)) => entity.as_ref().clone(),
        _ => panic!("command block entity should exist after the scheduled callback"),
    };
    assert_eq!(command_entity(&world), command_entity(&native));
}

#[test]
fn command_block_without_entity_matches_interpreter_initialization() {
    let command_pos = BlockPos::new(8, 20, 5);
    let lever_pos = command_pos.offset(BlockFace::North);
    let mut command = Block::from_name("command_block").unwrap();
    command.set_properties(std::collections::HashMap::from([("facing", "east")]));
    let mut world = empty_world();
    world.set_block(command_pos, command);
    world.set_block(lever_pos, lever(false));
    let mut native = empty_world();
    native.set_block(command_pos, command);
    native.set_block(lever_pos, lever(false));
    let mut backend = recording_graph(&world, vec![]);

    backend.use_block(lever_pos).unwrap();
    set_native_lever_as_interaction(&mut native, lever_pos, true);
    backend.tick_with_world(&mut world).unwrap();
    native.tick_interpreted();

    let command_entity = |world: &PlotWorld| match world.get_block_entity(command_pos) {
        Some(BlockEntity::CommandBlock(entity)) => entity.as_ref().clone(),
        _ => panic!("command block entity should be initialized by its first update"),
    };
    assert_eq!(command_entity(&world), command_entity(&native));
    assert!(matches!(
        world.get_block_entity(command_pos),
        Some(BlockEntity::CommandBlock(entity)) if entity.powered
    ));
}

#[test]
fn hostless_tick_preserves_work_that_can_activate_a_command() {
    let repeater_pos = BlockPos::new(6, 20, 5);
    let lever_pos = repeater_pos.offset(BlockFace::East);
    let command_pos = repeater_pos.offset(BlockFace::West);
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            facing: BlockDirection::East,
            ..Default::default()
        },
    };
    let mut command = Block::from_name("command_block").unwrap();
    command.set_properties(std::collections::HashMap::from([("facing", "east")]));
    let entry = TickEntry {
        pos: repeater_pos,
        block_type: Some(repeater.registry_id()),
        ticks_left: 0,
        tick_priority: TickPriority::NanoTick,
    };
    let make_world = || {
        let mut world = empty_world();
        world.set_block(lever_pos, lever(true));
        world.set_block(repeater_pos, repeater);
        world.set_block(command_pos, command);
        world.set_block_entity(
            command_pos,
            BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
                command: "say callback".into(),
                ..Default::default()
            })),
        );
        world
    };
    let world = make_world();
    let mut backend = GraphBackend::compile(&world, BOUNDS, vec![entry], Arc::default()).unwrap();
    let pending_before = backend.scheduled_ticks();
    let clock_before = backend.clock.logical_tick;
    assert_eq!(backend.tick(), Err(GraphExecutionError::CommandHostUnavailable { position: command_pos }));
    assert_eq!(backend.scheduled_ticks(), pending_before);
    assert_eq!(backend.clock.logical_tick, clock_before);
    assert!(!state_at(&backend, repeater_pos).powered);

    let mut world_with_host = make_world();
    let mut with_host = GraphBackend::compile(
        &world_with_host,
        BOUNDS,
        vec![TickEntry {
            pos: repeater_pos,
            block_type: Some(repeater.registry_id()),
            ticks_left: 0,
            tick_priority: TickPriority::NanoTick,
        }],
        Arc::default(),
    )
    .unwrap();
    with_host.set_test_recording(true);
    with_host.tick_with_world(&mut world_with_host).unwrap();
    assert!(state_at(&with_host, repeater_pos).powered);
    assert_eq!(
        world_with_host.command_output().collect::<Vec<_>>(),
        [r#"{"text":"[@] callback"}"#],
        "{}",
        with_host.trace().join("\n")
    );
}

#[test]
fn selected_trapdoor_rejection_preserves_world_work_and_compiler_activation() {
    let piston_pos = BlockPos::new(5, 20, 5);
    let trapdoor_pos = BlockPos::new(8, 20, 5);
    let command_pos = BlockPos::new(10, 20, 5);
    let mut world = empty_world();
    world.set_block(
        piston_pos,
        Block::Piston {
            piston: mchprs_blocks::blocks::RedstonePiston {
                facing: mchprs_blocks::BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    let trapdoor = Block::from_name("iron_trapdoor").unwrap();
    world.set_block(trapdoor_pos, trapdoor);
    let mut command = Block::from_name("command_block").unwrap();
    command.set_properties(std::collections::HashMap::from([("facing", "east")]));
    world.set_block(command_pos, command);
    world.set_block_entity(
        command_pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say must-not-run".into(),
            ..Default::default()
        })),
    );
    world.schedule_half_tick(trapdoor_pos, 2, TickPriority::Normal);
    let before_ticks = world.scheduler().iter_entries().collect::<Vec<_>>();
    let before_entity = match world.get_block_entity(command_pos) {
        Some(BlockEntity::CommandBlock(entity)) => entity.as_ref().clone(),
        _ => panic!("command block entity is present before compilation"),
    };
    let before_clock = world.piston_state().logical_tick;
    let before_events = world.piston_state().events.len();

    let mut compiler = crate::redpiler::Compiler::default();
    let result = compiler.compile(
        &world,
        BOUNDS,
        crate::redpiler::CompilerOptions::default(),
        before_ticks.clone(),
        Arc::default(),
    );
    let error = result.unwrap_err().to_string();
    assert!(error.contains("trapdoor"), "{error}");
    assert!(error.contains(&format!("{trapdoor_pos:?}")), "{error}");
    assert_eq!(world.get_block(trapdoor_pos), trapdoor);
    assert!(matches!(
        world.get_block_entity(command_pos),
        Some(BlockEntity::CommandBlock(entity)) if entity.as_ref() == &before_entity
    ));
    assert_eq!(world.scheduler().iter_entries().collect::<Vec<_>>(), before_ticks);
    assert_eq!(world.piston_state().logical_tick, before_clock);
    assert_eq!(world.piston_state().events.len(), before_events);
    assert!(world.command_output().next().is_none());
    assert!(!compiler.is_active());
    assert_eq!(compiler.backend_kind(), None);
    assert!(compiler.stats().is_none());
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
    let scheduler = entries.clone().into_iter().collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    let mut backend = recording_graph(&native, entries);

    assert_eq!(backend.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
    backend.tick().unwrap();
    native.tick_interpreted();
    assert_eq!(backend.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
    assert_eq!(backend.scheduled_ticks()[0].ticks_left, 1);

    backend.tick().unwrap();
    native.tick_interpreted();
    assert!(backend.scheduled_ticks().is_empty());
    assert!(native.scheduler().iter_entries().next().is_none());
    assert!(backend.trace().iter().any(|line| line == &format!("stale {position:?} skipped")));
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
    graph_world.set_block_entity(comparator_pos, BlockEntity::Comparator { output_strength: 15 });
    graph_world.set_block(lamp_pos, Block::RedstoneLamp { lit: false });
    let mut backend = recording_graph(&graph_world, ticks.clone());

    let scheduler = ticks.into_iter().collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(source_pos, lever(true));
    native.set_block(comparator_pos, comparator);
    native.set_block_entity(comparator_pos, BlockEntity::Comparator { output_strength: 15 });
    native.set_block(lamp_pos, Block::RedstoneLamp { lit: false });

    backend.tick().unwrap();
    native.tick_interpreted();
    backend.flush(&mut graph_world, false);
    assert!(
        state_at(&backend, lamp_pos).powered,
        "unchanged comparator output still delivers its callback"
    );
    assert!(matches!(native.get_block(lamp_pos), Block::RedstoneLamp { lit: true }));
    let comparator_node = backend.positions[&comparator_pos];
    let route = backend.program.nodes()[comparator_node.index()].output_route.unwrap();
    assert!(backend.trace().iter().any(|item| item.starts_with(&format!("route {}:", route.index()))));
    assert_eq!(backend.trace().iter().filter(|line| line.as_str() == "clock.advance").count(), 1);
    assert!(
        backend.trace().iter().position(|line| line.starts_with("dispatch ")).unwrap()
            < backend.trace().iter().position(|line| line == "clock.advance").unwrap()
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
    let mut backend = recording_graph(&graph_world, vec![]);
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
        backend.flush(&mut graph_world, false);
        assert_eq!(state_at(&backend, repeater_pos).powered, expected);
        assert_eq!(graph_world.get_block(repeater_pos), native.get_block(repeater_pos));
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
        world.set_block_entity(side_pos, BlockEntity::Comparator { output_strength: 15 });
    }
    let mut backend = recording_graph(&graph_world, vec![]);
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
    let mut backend = recording_graph(&graph_world, vec![]);
    for (powered, lit) in [(true, true), (false, true), (true, false)] {
        backend.use_block(LEVER_POS).unwrap();
        set_native_lever(&mut native, LEVER_POS, powered);
        backend.flush(&mut graph_world, false);
        assert_eq!(graph_world.get_block(bulb_pos), native.get_block(bulb_pos));
        assert_eq!(graph_world.get_block(bulb_pos).copper_bulb_state(), Some((lit, powered)));
        assert_eq!(state_at(&backend, bulb_pos).powered, powered);
        assert_eq!(state_at(&backend, bulb_pos).output_strength, if lit { 15 } else { 0 });
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
        world.set_block_entity(comparator_pos, BlockEntity::Comparator { output_strength: 0 });
    }
    let mut backend = recording_graph(&graph_world, vec![]);
    let comparator_node = backend.positions[&comparator_pos];

    backend.use_block(lever_pos).unwrap();
    let native_trace =
        crate::redstone::instant_piston_tests::capture_at(&[bulb_pos, comparator_pos], || set_native_lever(&mut native, lever_pos, true));
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
            let DeliveryKind::Update { direction } = kind else { unreachable!() };
            (*position, *direction)
        })
        .collect();
    assert_eq!(graph_callbacks, native_callbacks);
    assert_eq!(native_callbacks.iter().filter(|(pos, _)| *pos == comparator_pos).count(), 1);
    assert_eq!(
        backend.comparator_updates.iter().filter(|&&position| position == comparator_pos).count(),
        1
    );
    assert_eq!(backend.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
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
    let mut backend = recording_graph(&graph_world, vec![]);

    backend.use_block(lever_pos).unwrap();
    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::East);
    for expected in [true, false] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world, false);
        assert_eq!(state_at(&backend, torch_pos).powered, expected);
        assert_eq!(graph_world.get_block(torch_pos), native.get_block(torch_pos));
    }

    backend.use_block(lever_pos).unwrap();
    set_native_lever_with_support(&mut native, lever_pos, false, BlockFace::East);
    for expected in [false, true] {
        backend.tick().unwrap();
        native.tick_interpreted();
        backend.flush(&mut graph_world, false);
        assert_eq!(state_at(&backend, torch_pos).powered, expected);
        assert_eq!(graph_world.get_block(torch_pos), native.get_block(torch_pos));
    }
}

fn frozen_clipboard(name: &str) -> WorldEditClipboard {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data/instant-pistons-io");
    let manifest: Value = serde_json::from_slice(&std::fs::read(directory.join("download-manifest.json")).unwrap()).unwrap();
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
        matches!(block, Block::RedstoneTorch { .. } | Block::RedstoneWallTorch { .. })
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
    let full = recording_graph(&world, vec![]);
    assert_eq!(full.input_strength(full.positions[&lamp], InputPort::Main), 15);
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
            backend.update_component(node, DeliveryKind::Update { direction: None }, None).unwrap();
            redstone::update(world.get_block(lamp), &mut world, lamp, None);
            assert_eq!(backend.node_block(node), world.get_block(lamp));
        }
    }
}

#[test]
fn imported_stale_and_inert_ticks_are_retained_or_explicitly_rejected() {
    let repeater = frozen_component("ADDER_1BIT.schem", |block| matches!(block, Block::RedstoneRepeater { .. }));
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
    let scheduler = entries.clone().into_iter().collect::<TickScheduler<ScheduledBlockTick>>();
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
                message.contains("tick") && entries.iter().any(|entry| message.contains(&format!("{:?}", entry.pos))),
                "rejection must identify the unbound scheduled target: {message}"
            );
            assert_eq!(native.scheduler().iter_entries().collect::<Vec<_>>(), before);
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
                assert_eq!(backend.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
            }
        }
    }
}

#[test]
fn removed_repeater_tick_keeps_its_deadline_until_native_stale_dispatch() {
    let repeater = frozen_component("ADDER_1BIT.schem", |block| matches!(block, Block::RedstoneRepeater { .. }));
    let removed = NEXT_POS;
    let entry = TickEntry {
        pos: removed,
        block_type: Some(repeater.registry_id()),
        ticks_left: 4,
        tick_priority: TickPriority::High,
    };
    let scheduler = vec![entry].into_iter().collect::<TickScheduler<ScheduledBlockTick>>();
    let mut native = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], scheduler);
    native.set_block(LEVER_POS, lever(false));
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    let before = native.scheduler().iter_entries().collect::<Vec<_>>();
    match GraphBackend::compile(&native, BOUNDS, before.clone(), Arc::default()) {
        Err(error) => {
            let message = error.to_string();
            assert!(message.contains("tick") && message.contains(&format!("{removed:?}")), "{message}");
            assert_eq!(native.scheduler().iter_entries().collect::<Vec<_>>(), before);
        }
        Ok(mut backend) => {
            assert_eq!(backend.scheduled_ticks(), before);
            let mut dispatches = Vec::new();
            for _ in 0..4 {
                let records = capture_at(&[removed], || native.tick_interpreted());
                dispatches.push(records.iter().filter(|record| record["kind"] == "scheduled_tick").count());
                backend.tick().unwrap();
                assert_eq!(backend.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
                assert_eq!(backend.clock.logical_tick, native.piston_state().logical_tick);
            }
            assert_eq!(dispatches, [0, 0, 0, 1]);
            assert_eq!(native.get_block(removed), Block::Air);
        }
    }
}

#[test]
fn observer_shape_then_explicit_update_matches_native_for_every_orientation() {
    let template = frozen_component("INSTANT_OBSERVER.schem", |block| matches!(block, Block::Observer { .. }));
    let Block::Observer { observer: template } = template else {
        unreachable!()
    };
    let position = BlockPos::new(7, 20, 7);
    let mut mismatches = Vec::new();
    let original_facing = BlockFace::from(template.facing);
    let facings = std::iter::once(original_facing).chain(BlockFace::values().into_iter().filter(|face| *face != original_facing));
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
            let mut backend = recording_graph(&native, vec![]);
            let node = backend.positions[&position];
            let shape_direction = if watches_changed_cell { facing.opposite() } else { facing };
            crate::interaction::change(observer, &mut native, position, shape_direction);
            backend
                .update_component(node, DeliveryKind::Shape { direction: shape_direction }, None)
                .unwrap();
            if backend.is_pending(node) != native.pending_tick_at(position) {
                mismatches.push((facing, watches_changed_cell, "after_shape", 0));
            }
            let update_direction = shape_direction.opposite();
            redstone::update(native.get_block(position), &mut native, position, Some(update_direction));
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
    let control_face = [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West]
        .into_iter()
        .find(|face| support.offset(*face) != position)
        .unwrap();
    native.set_block(
        support.offset(control_face),
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, control_face.unwrap_direction(), true),
        },
    );
    let mut backend = recording_graph(&native, vec![]);
    let node = backend.positions[&position];
    assert_eq!(backend.input_strength(node, InputPort::Main), 15);
    assert!(crate::interaction::is_valid_position(torch, &native, position));
    crate::interaction::change(torch, &mut native, position, BlockFace::East);
    backend
        .update_component(node, DeliveryKind::Shape { direction: BlockFace::East }, None)
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
    let template = frozen_component("ADDER_1BIT.schem", |block| matches!(block, Block::RedstoneRepeater { .. }));
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
        let mut backend = recording_graph(&native, vec![]);
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
        let native_strength = redstone::source_strength(native.get_block(position), &native, position);
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
        let expected = (native.get_block(position), native_strength, native.get_block(lamp));
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
    let faces = [BlockFace::North, BlockFace::South, BlockFace::East, BlockFace::West];
    let mut comparators = Vec::new();
    for face in faces {
        let support = bulb.offset(face);
        native.set_block(support, Block::Stone {});
        let position = support.offset(face);
        native.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(face.opposite().unwrap_direction(), ComparatorMode::Compare, false),
            },
        );
        native.set_block_entity(position, BlockEntity::Comparator { output_strength: 0 });
        comparators.push(position);
    }
    let mut backend = recording_graph(&native, vec![]);
    let records = capture_at(&comparators, || set_native_lever(&mut native, control, true));
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
    assert_eq!(actual, expected, "same final output does not justify additional far callbacks");
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
            matches!(GraphBackend::compile(&world, BOUNDS, vec![], Arc::default()), Ok(_)),
            "isolated sticky={sticky} piston should be admitted"
        );
    }
}

fn opposing_shared_pistons() -> (PlotWorld, PlotWorld, [BlockPos; 2], [BlockPos; 2], BlockPos) {
    let mut native = empty_world();
    let mut graph_world = empty_world();
    let bases = [BlockPos::new(5, 20, 5), BlockPos::new(5, 20, 9)];
    let facings = [BlockFacing::South, BlockFacing::North];
    let near = [bases[0].offset(BlockFace::South), bases[1].offset(BlockFace::North)];
    let resource = BlockPos::new(5, 20, 7);
    for world in [&mut native, &mut graph_world] {
        for (base, facing) in bases.into_iter().zip(facings) {
            let piston = mchprs_blocks::blocks::RedstonePiston {
                facing,
                sticky: true,
                extended: true,
            };
            world.set_block(base, Block::Piston { piston });
            world.set_block(base.offset(facing.into()), Block::PistonHead { head: piston.into() });
        }
        world.set_block(resource, Block::RedstoneBlock);
    }
    (native, graph_world, bases, near, resource)
}

#[test]
fn shared_payload_opposite_direction_does_not_request_early_retraction() {
    for extending_actor in [0usize, 1] {
        let retracting_actor = 1 - extending_actor;
        let (mut native, mut graph_world, bases, near, far) = opposing_shared_pistons();
        let controls = bases.map(|base| base.offset(BlockFace::East));
        for world in [&mut native, &mut graph_world] {
            for index in 0..2 {
                let Block::Piston { mut piston } = world.get_block(bases[index]) else {
                    unreachable!();
                };
                piston.extended = false;
                world.set_block(bases[index], Block::Piston { piston });
                world.set_block(near[index], if index == extending_actor { Block::RedstoneBlock } else { Block::Air });
                world.set_block(controls[index].offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(controls[index], lever(false));
            }
            world.set_block(far, Block::Air);
        }
        let mut backend = recording_graph(&graph_world, vec![]);
        assert_eq!(backend.program.resources().len(), 1);
        for index in [extending_actor, retracting_actor] {
            set_native_lever(&mut native, controls[index], true);
            backend.use_block(controls[index]).unwrap();
        }
        tick_native_bounded(&mut native);
        backend.tick().unwrap();
        assert_resource_invariants(&backend);

        set_native_lever(&mut native, controls[retracting_actor], false);
        backend.use_block(controls[retracting_actor]).unwrap();
        let native_request = native.piston_state().events.back().unwrap();
        assert_eq!(native_request.pos, bases[retracting_actor]);
        assert_eq!(native_request.action, mchprs_world::PistonAction::Retract);
        let graph_request = backend.piston_requests.back().unwrap();
        assert_eq!(graph_request.piston, piston_id_at(&backend, bases[retracting_actor]));
        assert_eq!(graph_request.action, native_request.action);
    }
}

#[test]
fn shared_payload_deadline_reversal_executes_notified_actor_in_same_tick() {
    for extending_actor in [0usize, 1] {
        let notified_actor = 1 - extending_actor;
        let (mut native, mut graph_world, bases, near, far) = opposing_shared_pistons();
        let control = bases[extending_actor].offset(BlockFace::East);
        for world in [&mut native, &mut graph_world] {
            let Block::Piston { mut piston } = world.get_block(bases[extending_actor]) else {
                unreachable!();
            };
            piston.extended = false;
            world.set_block(bases[extending_actor], Block::Piston { piston });
            world.set_block(near[extending_actor], Block::RedstoneBlock);
            world.set_block(far, Block::Air);
            world.set_block(control.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(control, lever(false));
        }
        let mut backend = recording_graph(&graph_world, vec![]);
        assert_eq!(backend.program.resources().len(), 1);
        set_native_lever(&mut native, control, true);
        backend.use_block(control).unwrap();
        for _ in 0..2 {
            tick_native_bounded(&mut native);
            backend.tick().unwrap();
            assert_resource_invariants(&backend);
        }

        set_native_lever(&mut native, control, false);
        backend.use_block(control).unwrap();
        tick_native_bounded(&mut native);
        backend.tick().unwrap();
        assert_resource_invariants(&backend);
        assert!(matches!(native.get_block(bases[notified_actor]), Block::MovingPiston { .. }));
        assert_eq!(
            piston_state_at(&backend, bases[notified_actor]).availability,
            PistonAvailability::BasePending,
            "the notified actor must retract in the resource's interruption tick"
        );
        assert!(native.piston_state().events.is_empty());
        assert!(backend.piston_requests.is_empty());
        assert_eq!(backend.state.resources[0].at, contact_at(&backend, near[notified_actor]));
        assert!(!backend.state.resources[0].enabled);

        for _ in 0..3 {
            tick_native_bounded(&mut native);
            backend.tick().unwrap();
            assert_resource_invariants(&backend);
        }
        for position in [bases[0], bases[1], near[0], near[1], far] {
            assert_eq!(piston_contact_block(&backend, position), native.get_block(position));
        }
    }
}

#[test]
fn shared_payload_actor_orders_select_the_first_retracting_contact() {
    for order in [[0usize, 1usize], [1, 0]] {
        let (mut native, graph_world, bases, near, resource_pos) = opposing_shared_pistons();
        let mut backend = GraphBackend::compile(&graph_world, BOUNDS, vec![], Arc::default())
            .unwrap_or_else(|error| panic!("shared two-actor fixture rejected: {error}"));
        backend.set_test_recording(true);

        for actor_index in order {
            let base = bases[actor_index];
            let Block::Piston { piston } = native.get_block(base) else {
                unreachable!();
            };
            redstone::piston::update_piston_state(&mut native, piston, base);
            let piston_id = backend
                .program
                .pistons()
                .iter()
                .find(|spec| backend.program.nodes()[spec.node.index()].position == Some(base))
                .map(|spec| match backend.program.nodes()[spec.node.index()].kind {
                    CircuitNodeKind::Piston(id) => id,
                    _ => unreachable!(),
                })
                .unwrap();
            backend.sample_piston(piston_id).unwrap();
            for _ in 0..3 {
                tick_shared_and_compare_callbacks(&mut native, &mut backend);
            }
            for position in [bases[0], bases[1], near[0], near[1], resource_pos] {
                assert_eq!(
                    piston_contact_block(&backend, position),
                    native.get_block(position),
                    "contact projection at {position:?} for order {order:?}"
                );
            }
        }

        let expected = near[order[0]];
        let resource = backend.state.resources[0];
        assert_eq!(
            resource.at,
            backend
                .program
                .contacts()
                .iter()
                .position(|c| c.position == expected)
                .map(|i| ContactId::new(i as u32))
                .unwrap()
        );
        assert!(resource.enabled);
        assert_eq!(backend.state.pistons.iter().filter(|state| !state.extended).count(), 2);
    }
}

#[test]
fn shared_payload_overlapping_repeated_triggers_keep_fifo_and_conserve_one_token() {
    for order in [[0usize, 1usize], [1, 0]] {
        let (mut native, graph_world, bases, near, resource_pos) = opposing_shared_pistons();
        let mut backend = GraphBackend::compile(&graph_world, BOUNDS, vec![], Arc::default())
            .unwrap_or_else(|error| panic!("shared two-actor fixture rejected: {error}"));
        backend.set_test_recording(true);

        for actor_index in order {
            let base = bases[actor_index];
            let Block::Piston { piston } = native.get_block(base) else {
                unreachable!();
            };
            for _ in 0..2 {
                redstone::piston::update_piston_state(&mut native, piston, base);
                backend.sample_piston(piston_id_at(&backend, base)).unwrap();
            }
        }

        let native_order: Vec<_> = native.piston_state().events.iter().map(|event| event.pos).collect();
        let graph_order: Vec<_> = backend
            .piston_requests
            .iter()
            .map(|request| {
                let spec = backend.program.pistons()[request.piston.index()];
                backend.program.nodes()[spec.node.index()].position.unwrap()
            })
            .collect();
        assert_eq!(native_order, order.map(|index| bases[index]));
        assert_eq!(graph_order, native_order);

        for _ in 0..3 {
            tick_shared_and_compare_callbacks(&mut native, &mut backend);
        }
        assert_eq!(backend.state.resources.len(), 1);
        assert!(backend.state.resources[0].enabled);
        assert_eq!(native.get_block(resource_pos), Block::Air);
        let native_tokens = [near[0], near[1]]
            .into_iter()
            .filter(|position| native.get_block(*position) == Block::RedstoneBlock)
            .count();
        assert_eq!(native_tokens, 1);
        assert_eq!(piston_contact_block(&backend, near[order[0]]), Block::RedstoneBlock);
        for position in [bases[0], bases[1], near[0], near[1], resource_pos] {
            assert_eq!(
                piston_contact_block(&backend, position),
                native.get_block(position),
                "contact projection at {position:?} for FIFO order {order:?}"
            );
        }
    }
}

#[test]
fn three_sticky_actors_share_one_conserved_resource() {
    let mut native = empty_world();
    let mut graph_world = empty_world();
    let bases = [
        (BlockPos::new(3, 20, 5), BlockFacing::East),
        (BlockPos::new(7, 20, 5), BlockFacing::West),
        (BlockPos::new(5, 20, 3), BlockFacing::South),
    ];
    let resource_pos = BlockPos::new(5, 20, 5);
    for world in [&mut native, &mut graph_world] {
        for (base, facing) in bases {
            let piston = mchprs_blocks::blocks::RedstonePiston {
                facing,
                sticky: true,
                extended: true,
            };
            world.set_block(base, Block::Piston { piston });
            world.set_block(base.offset(facing.into()), Block::PistonHead { head: piston.into() });
        }
        world.set_block(resource_pos, Block::RedstoneBlock);
    }
    let mut backend = GraphBackend::compile(&graph_world, BOUNDS, vec![], Arc::default())
        .unwrap_or_else(|error| panic!("three-actor shared fixture rejected: {error}"));
    backend.set_test_recording(true);
    assert_eq!(backend.program.resources().len(), 1);
    assert_eq!(backend.program.resources()[0].contacts.len(), 4);

    let first = 2usize;
    let Block::Piston { piston } = native.get_block(bases[first].0) else {
        unreachable!();
    };
    redstone::piston::update_piston_state(&mut native, piston, bases[first].0);
    backend.sample_piston(piston_id_at(&backend, bases[first].0)).unwrap();
    for _ in 0..3 {
        tick_shared_and_compare_callbacks(&mut native, &mut backend);
    }
    let near = bases[first].0.offset(bases[first].1.into());
    assert_eq!(native.get_block(near), Block::RedstoneBlock);
    assert_eq!(piston_contact_block(&backend, near), Block::RedstoneBlock);
    assert!(backend.state.resources[0].enabled);
    assert_eq!(backend.state.resources.len(), 1);
}

#[test]
fn shared_resource_drop_and_recapture_ignore_the_obsolete_actor_event() {
    let (mut native, mut graph_world, bases, near, resource_pos) = opposing_shared_pistons();
    let controls = [bases[0].offset(BlockFace::East), bases[1].offset(BlockFace::East)];
    for world in [&mut native, &mut graph_world] {
        for (index, control) in controls.into_iter().enumerate() {
            world.set_block(control.offset(BlockFace::Bottom), Block::Stone {});
            world.set_block(control, lever(index == 1));
        }
    }
    let mut backend = GraphBackend::compile(&graph_world, BOUNDS, vec![], Arc::default())
        .unwrap_or_else(|error| panic!("shared drop/recapture fixture rejected: {error}"));
    backend.set_test_recording(true);

    let Block::Piston { piston } = native.get_block(bases[0]) else {
        unreachable!();
    };
    redstone::piston::update_piston_state(&mut native, piston, bases[0]);
    backend.sample_piston(piston_id_at(&backend, bases[0])).unwrap();
    for _ in 0..3 {
        tick_shared_and_compare_callbacks(&mut native, &mut backend);
    }
    assert_eq!(native.get_block(near[0]), Block::RedstoneBlock);

    set_shared_lever_and_compare_callbacks(&mut native, &mut backend, controls[0], true);
    assert!(matches!(
        native.piston_state().events.back(),
        Some(event) if event.action == mchprs_world::PistonAction::Extend
    ));
    tick_shared_and_compare_callbacks(&mut native, &mut backend);
    assert!(!backend.state.resources[0].enabled);
    assert_eq!(backend.state.resources[0].at, contact_at(&backend, resource_pos));

    set_shared_lever_and_compare_callbacks(&mut native, &mut backend, controls[0], false);
    assert!(matches!(
        native.piston_state().events.back(),
        Some(event) if event.action == mchprs_world::PistonAction::RetractWithoutPull
    ));
    assert!(
        backend.piston_requests.iter().any(|request| {
            request.piston == piston_id_at(&backend, bases[0]) && request.action == mchprs_world::PistonAction::RetractWithoutPull
        })
    );
    tick_shared_and_compare_callbacks(&mut native, &mut backend);
    assert_eq!(native.get_block(resource_pos), Block::RedstoneBlock);
    assert_eq!(piston_contact_block(&backend, resource_pos), Block::RedstoneBlock);
    assert!(backend.state.resources[0].enabled);

    set_shared_lever_and_compare_callbacks(&mut native, &mut backend, controls[1], false);
    tick_shared_and_compare_callbacks(&mut native, &mut backend);
    assert_eq!(native.get_block(resource_pos), Block::Air);
    assert!(matches!(native.get_block(near[1]), Block::MovingPiston { .. }));
    assert_eq!(backend.state.resources[0].at, contact_at(&backend, near[1]));
    assert!(!backend.state.resources[0].enabled);
    assert!(backend.trace().iter().any(|line| line.contains("resource stale")));
    assert!(backend.trace().iter().any(|line| line.contains("piston stale")));

    for _ in 0..3 {
        tick_shared_and_compare_callbacks(&mut native, &mut backend);
    }
    assert_eq!(native.get_block(near[1]), Block::RedstoneBlock);
    assert_eq!(piston_contact_block(&backend, near[1]), Block::RedstoneBlock);
    assert!(backend.state.resources[0].enabled);
    assert_eq!(native.get_block(resource_pos), Block::Air);
    for position in [bases[0], bases[1], near[0], near[1], resource_pos] {
        assert_eq!(
            piston_contact_block(&backend, position),
            native.get_block(position),
            "contact projection at {position:?} after drop and recapture"
        );
    }
}

#[test]
fn stale_resource_publication_does_not_cancel_a_valid_actor_publication() {
    let (_, graph_world, bases, _, resource_pos) = opposing_shared_pistons();
    let mut backend = GraphBackend::compile(&graph_world, BOUNDS, vec![], Arc::default())
        .unwrap_or_else(|error| panic!("shared two-actor fixture rejected: {error}"));
    backend.set_test_recording(true);
    let actor = piston_id_at(&backend, bases[0]);
    backend.retract_connections(actor, false).unwrap();
    let event = backend.deferred_pistons.pop_front().unwrap();
    assert_eq!(backend.state.pistons[actor.index()].availability, PistonAvailability::BasePending);

    // Model a legal recapture at the shared contact before the original restore arrives.
    backend.set_resource_contact(ResourceId::new(0), contact_at(&backend, resource_pos), true);
    backend.apply_deferred_event(event).unwrap();
    assert_resource_invariants(&backend);

    assert_eq!(backend.state.pistons[actor.index()].availability, PistonAvailability::Ready);
    assert!(!backend.state.pistons[actor.index()].extended);
    assert_eq!(backend.state.resources[0].at, contact_at(&backend, resource_pos));
    assert!(backend.state.resources[0].enabled);
    assert!(backend.trace().iter().any(|line| line.contains("resource stale")));
    assert!(!backend.trace().iter().any(|line| line.contains("piston stale")));
}

#[test]
fn adder_inhibition_matches_native_for_a_bounded_shared_resource_episode() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let clipboard = frozen_clipboard("ADDER_1BIT.schem");
    let dimensions = (clipboard.size_x, clipboard.size_y, clipboard.size_z);
    let origin = BlockPos::new(40, 30, 40);
    let paste_anchor = origin + BlockPos::new(clipboard.offset_x, clipboard.offset_y, clipboard.offset_z);
    let bounds = (
        origin,
        origin + BlockPos::new(dimensions.0 as i32 - 1, dimensions.1 as i32 - 1, dimensions.2 as i32 - 1),
    );
    let mut native = fixture_world();
    let mut graph_world = fixture_world();
    for world in [&mut native, &mut graph_world] {
        paste_clipboard(world, &clipboard, paste_anchor, false);
    }
    let mut backend =
        GraphBackend::compile(&graph_world, bounds, vec![], Arc::default()).unwrap_or_else(|error| panic!("bounded adder fixture rejected: {error}"));
    let mut component_positions = non_resource_component_positions(&backend);
    component_positions.sort_by_key(|position| (position.x, position.y, position.z));
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-1-1-0")
        .unwrap();

    let compare_components = |backend: &mut GraphBackend, native: &PlotWorld, graph_world: &mut PlotWorld, boundary: &str| {
        assert_resource_invariants(backend);
        backend.flush(graph_world, false);
        for &position in &component_positions {
            assert_eq!(
                graph_world.get_block(position),
                native.get_block(position),
                "adder component at {position:?} diverged at {boundary}"
            );
        }
    };
    let advance = |backend: &mut GraphBackend, native: &mut PlotWorld, graph_world: &mut PlotWorld, boundary: &str| {
        native.tick_interpreted();
        backend.tick().unwrap();
        compare_components(backend, native, graph_world, boundary);
    };

    for (index, action) in case["actions"].as_array().unwrap().iter().enumerate() {
        match action["op"].as_str().unwrap() {
            "lever" => {
                let position = origin
                    + BlockPos::new(
                        action["pos"][0].as_i64().unwrap() as i32,
                        action["pos"][1].as_i64().unwrap() as i32,
                        action["pos"][2].as_i64().unwrap() as i32,
                    );
                let powered = action["powered"].as_bool().unwrap();
                let Block::Lever { lever } = graph_world.get_block(position) else {
                    panic!("expected adder control lever at {position:?}");
                };
                if lever.powered != powered {
                    backend.use_block(position).unwrap();
                }
                set_native_lever_as_interaction(&mut native, position, powered);
                compare_components(&mut backend, &native, &mut graph_world, "control action");
            }
            "wait_ready" | "wait" => {
                for tick in 0..action["ticks"].as_u64().unwrap() {
                    advance(&mut backend, &mut native, &mut graph_world, &format!("setup action {index}, tick {tick}"));
                }
            }
            operation => panic!("unexpected adder operation {operation}"),
        }
    }
    for tick in 0..case["ticks"].as_u64().unwrap() {
        advance(&mut backend, &mut native, &mut graph_world, &format!("inhibition response tick {tick}"));
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

fn piston_id_at(backend: &GraphBackend, position: BlockPos) -> PistonId {
    let spec = backend
        .program
        .pistons()
        .iter()
        .find(|spec| backend.program.nodes()[spec.node.index()].position == Some(position))
        .unwrap();
    match backend.program.nodes()[spec.node.index()].kind {
        CircuitNodeKind::Piston(id) => id,
        _ => unreachable!(),
    }
}

fn contact_at(backend: &GraphBackend, position: BlockPos) -> ContactId {
    backend
        .program
        .contacts()
        .iter()
        .position(|contact| contact.position == position)
        .map(|index| ContactId::new(index as u32))
        .unwrap()
}

pub(super) fn assert_resource_invariants(backend: &GraphBackend) {
    let mut selected = Vec::new();
    for (index, state) in backend.state.resources.iter().enumerate() {
        let spec = &backend.program.resources()[index];
        assert!(spec.contacts.contains(&state.at));
        assert!(!selected.contains(&state.at), "two resources selected one contact");
        selected.push(state.at);
    }
    for contact in backend.program.contacts() {
        assert_eq!(
            contact
                .variants
                .iter()
                .filter(|variant| backend.guard_matches(variant.when, None))
                .count(),
            1,
            "contact {:?} must select exactly one variant",
            contact.position
        );
    }
}

fn non_resource_component_positions(backend: &GraphBackend) -> Vec<BlockPos> {
    let resource_contacts: Vec<_> = backend
        .program
        .resources()
        .iter()
        .flat_map(|resource| resource.contacts.iter())
        .map(|contact| backend.program.contacts()[contact.index()].position)
        .collect();
    // These positions are projected through ResourceState, not component flushes.
    backend
        .positions
        .keys()
        .copied()
        .filter(|position| !resource_contacts.contains(position))
        .collect()
}

fn shared_callback_positions(backend: &GraphBackend) -> Vec<BlockPos> {
    let mut positions: Vec<_> = backend
        .positions
        .iter()
        .filter(|(_, node)| backend.has_semantic_callback(CallbackReceiver::Component(**node)))
        .map(|(position, _)| *position)
        .collect();
    for piston in backend.program.pistons() {
        for contact in [piston.base, piston.near] {
            let position = backend.program.contacts()[contact.index()].position;
            if !positions.contains(&position) {
                positions.push(position);
            }
        }
    }
    positions
}

fn tick_shared_and_compare_callbacks(native: &mut PlotWorld, backend: &mut GraphBackend) {
    let watched = shared_callback_positions(backend);
    let graph_start = backend.observed_operations.len();
    let native_trace = capture_at(&watched, || tick_native_bounded(native));
    backend.tick().unwrap();
    let native_callbacks = native_shared_callbacks_at(&native_trace, &watched);
    let graph_callbacks = graph_routed_callbacks_at(backend, graph_start, &watched);
    assert_eq!(
        graph_callbacks,
        native_callbacks,
        "shared callbacks at tick {}: {:#?}",
        backend.clock.logical_tick,
        backend.trace.iter().rev().take(24).collect::<Vec<_>>()
    );
    assert_resource_invariants(backend);
}

fn set_shared_lever_and_compare_callbacks(native: &mut PlotWorld, backend: &mut GraphBackend, position: BlockPos, powered: bool) {
    let watched = shared_callback_positions(backend);
    let graph_start = backend.observed_operations.len();
    let native_trace = capture_at(&watched, || set_native_lever(native, position, powered));
    backend.use_block(position).unwrap();
    let native_callbacks = native_shared_callbacks_at(&native_trace, &watched);
    let graph_callbacks = graph_routed_callbacks_at(backend, graph_start, &watched);
    assert_eq!(graph_callbacks, native_callbacks);
    assert_resource_invariants(backend);
}

fn tick_native_bounded(world: &mut PlotWorld) {
    let target_tick = world.piston_state().logical_tick + 1;
    for _ in 0..128 {
        world.nanotick_advance(1);
        if world.piston_state().logical_tick == target_tick && world.piston_state().phase == mchprs_world::AdvancePhase::BetweenTicks {
            return;
        }
    }
    panic!("native piston tick did not settle within 128 operations");
}

fn native_callbacks_at(trace: &[Value], positions: &[BlockPos]) -> Vec<(BlockPos, Option<BlockFace>)> {
    trace
        .iter()
        .filter(|entry| {
            entry["kind"] == "callback"
                && serde_json::from_value::<BlockPos>(entry["data"]["pos"].clone()).is_ok_and(|position| positions.contains(&position))
        })
        .map(|entry| {
            (
                serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
            )
        })
        .collect()
}

fn native_shared_callbacks_at(trace: &[Value], positions: &[BlockPos]) -> Vec<(BlockPos, Option<BlockFace>)> {
    trace
        .iter()
        .filter(|entry| {
            entry["kind"] == "callback"
                && matches!(
                    entry["data"]["block"].as_str(),
                    Some(
                        "redstone_wire"
                            | "redstone_torch"
                            | "redstone_wall_torch"
                            | "redstone_repeater"
                            | "redstone_comparator"
                            | "redstone_lamp"
                            | "observer"
                            | "piston"
                            | "sticky_piston"
                            | "piston_head"
                            | "copper_bulb"
                    )
                )
                && serde_json::from_value::<BlockPos>(entry["data"]["pos"].clone()).is_ok_and(|position| positions.contains(&position))
        })
        .map(|entry| {
            (
                serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
            )
        })
        .collect()
}

fn graph_callbacks_at(backend: &GraphBackend, start: usize, positions: &[BlockPos]) -> Vec<(BlockPos, Option<BlockFace>)> {
    backend.callback_order[start..]
        .iter()
        .filter(|(position, _)| positions.contains(position))
        .filter_map(|(position, kind)| match kind {
            DeliveryKind::Update { direction } => Some((*position, *direction)),
            DeliveryKind::Shape { .. } => None,
        })
        .collect()
}

fn graph_routed_callbacks_at(backend: &GraphBackend, start: usize, positions: &[BlockPos]) -> Vec<(BlockPos, Option<BlockFace>)> {
    backend.observed_operations[start..]
        .iter()
        .filter_map(|operation| match operation {
            GraphObservedOperation::Callback { target, direction } if positions.contains(target) => Some((*target, *direction)),
            _ => None,
        })
        .collect()
}

fn piston_contact_block(backend: &GraphBackend, position: BlockPos) -> Block {
    let site = backend
        .program
        .sites()
        .iter()
        .position(|site| site.position == position && matches!(site.source, SiteSource::Contact(_)))
        .unwrap();
    backend.resolve_site(SiteId::new(site as u32), None).unwrap().block
}

#[test]
fn same_value_quasi_updates_match_native_and_deduplicate_extension() {
    let (mut native, mut graph_world, base, _) = isolated_piston_pair(false);
    let quasi_source = base.offset(BlockFace::Top).offset(BlockFace::East);
    native.set_block(quasi_source, Block::RedstoneBlock);
    graph_world.set_block(quasi_source, Block::RedstoneBlock);
    let mut backend = recording_graph(&graph_world, vec![]);
    let piston_node = backend.program.pistons()[0].node;

    assert_eq!(backend.input_strength(piston_node, InputPort::Main), 0);
    assert!(backend.input_strength(piston_node, InputPort::Quasi) > 0);
    assert!(redstone::piston::should_piston_extend(&native, mchprs_blocks::BlockFacing::East, base));

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
    let mut backend = recording_graph(&graph_world, vec![]);

    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    set_native_lever_with_support(&mut native, lever_pos, false, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    tick_native_bounded(&mut native);
    backend.tick().unwrap();

    assert!(matches!(native.get_block(base), Block::Piston { piston } if !piston.extended));
    assert!(!piston_state_at(&backend, base).extended);
    assert!(backend.trace().iter().any(|line| line.contains("piston cancelled")));
}

#[test]
fn blocked_piston_lane_rejects_extension_at_sampling_and_acceptance() {
    let (mut native, mut graph_world, base, lever_pos) = isolated_piston_pair(false);
    let near = base.offset(BlockFace::East);
    let container = Block::from_name("barrel").unwrap();
    native.set_block(near, container);
    graph_world.set_block(near, container);
    let mut backend = recording_graph(&graph_world, vec![]);

    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    let piston = backend.program.pistons()[0].node;
    let id = match backend.program.nodes()[piston.index()].kind {
        CircuitNodeKind::Piston(id) => id,
        _ => unreachable!(),
    };
    backend.sample_piston(id).unwrap();
    assert!(backend.piston_requests.is_empty(), "sampling must reject the blocked lane");

    backend.enqueue_piston(PistonRequest {
        piston: id,
        action: mchprs_world::PistonAction::Extend,
    });
    backend.drain_piston_actions().unwrap();
    assert!(!piston_state_at(&backend, base).extended);
    assert!(!matches!(native.get_block(base), Block::Piston { piston } if piston.extended));
    assert!(native.piston_state().events.is_empty());
    assert!(backend.trace().iter().any(|line| line.contains("piston cancelled")));
}

#[test]
fn piston_early_reversal_matches_interpreter_and_invalidates_old_restoration() {
    let (mut native, mut graph_world, base, lever_pos) = isolated_piston_pair(true);
    let far = base.offset(BlockFace::East).offset(BlockFace::East);
    let far_lamp = far.offset(BlockFace::North);
    for world in [&mut native, &mut graph_world] {
        world.set_block(far_lamp, Block::RedstoneLamp { lit: false });
    }
    let mut backend = recording_graph(&graph_world, vec![]);
    let far_contact = match backend.program.pistons()[0].lane {
        PistonLane::SingleResource { far } => far,
        PistonLane::Empty => unreachable!(),
    };
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
    assert_eq!(piston_state_at(&backend, base).availability, PistonAvailability::BasePending);
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
        piston_contact_block(&backend, base.offset(BlockFace::East).offset(BlockFace::East)),
        Block::RedstoneBlock
    );
    tick_native_bounded(&mut native);
    backend.tick().unwrap();
    assert_eq!(piston_state_at(&backend, base).availability, PistonAvailability::BasePending);
    assert!(backend.trace().iter().any(|line| line.contains("piston stale")));
    tick_native_bounded(&mut native);
    backend.tick().unwrap();
    assert!(matches!(native.get_block(base), Block::Piston { piston } if !piston.extended));
    assert!(!piston_state_at(&backend, base).extended);
    assert_eq!(piston_state_at(&backend, base).availability, PistonAvailability::Ready);
    assert_eq!(piston_contact_block(&backend, base.offset(BlockFace::East)), Block::Air);
    assert_eq!(
        native.get_block(base.offset(BlockFace::East).offset(BlockFace::East)),
        piston_contact_block(&backend, base.offset(BlockFace::East).offset(BlockFace::East))
    );

    backend.flush(&mut graph_world, false);
    assert_eq!(graph_world.get_block(far_lamp), native.get_block(far_lamp));
    assert_eq!(native.get_block(far_lamp), Block::RedstoneLamp { lit: true });
    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    for tick in 1..=3 {
        let graph_start = backend.callback_order.len();
        let native_trace = capture_at(&[far_lamp], || tick_native_bounded(&mut native));
        backend.tick().unwrap();
        backend.flush(&mut graph_world, false);
        assert_eq!(
            graph_callbacks_at(&backend, graph_start, &[far_lamp]),
            native_callbacks_at(&native_trace, &[far_lamp]),
            "stationary far resource callback tick {tick}"
        );
        assert_eq!(graph_world.get_block(far_lamp), native.get_block(far_lamp));
        assert_eq!(backend.state.resources[0].at, far_contact);
        assert!(backend.state.resources[0].enabled);
    }
}

#[test]
fn ordinary_early_retraction_publishes_interrupted_head_before_base() {
    let (mut native, mut graph_world, base, lever_pos) = isolated_piston_pair(false);
    let far = base.offset(BlockFace::East).offset(BlockFace::East);
    let observer = Block::Observer {
        observer: RedstoneObserver {
            facing: BlockFacing::West,
            powered: false,
        },
    };
    native.set_block(far, observer);
    graph_world.set_block(far, observer);
    let mut backend = recording_graph(&graph_world, vec![]);

    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    tick_native_bounded(&mut native);
    backend.tick().unwrap();

    set_native_lever_with_support(&mut native, lever_pos, false, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    assert!(
        matches!(native.piston_state().events.back(), Some(event)
            if event.action == mchprs_world::PistonAction::Retract),
        "base={:?} events={:?}",
        native.get_block(base),
        native.piston_state().events
    );

    let graph_start = backend.callback_order.len();
    let native_trace = capture_at(&[far], || native.nanotick_advance(1));
    backend.tick().unwrap();
    let native_callbacks = native_callbacks_at(&native_trace, &[far]);
    let graph_callbacks = graph_callbacks_at(&backend, graph_start, &[far]);
    assert_eq!(graph_callbacks, native_callbacks);
    assert_eq!(native_callbacks.len(), 2, "interrupted head is removed before base retraction");
}

#[test]
fn piston_delayed_restoration_matches_interpreter() {
    let (mut native, mut graph_world, base, lever_pos) = isolated_piston_pair(true);
    let far = base.offset(BlockFace::East).offset(BlockFace::East);
    let far_lamp = far.offset(BlockFace::North);
    for world in [&mut native, &mut graph_world] {
        world.set_block(far_lamp, Block::RedstoneLamp { lit: false });
    }
    let mut backend = recording_graph(&graph_world, vec![]);
    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();

    for _ in 0..3 {
        tick_native_bounded(&mut native);
        backend.tick().unwrap();
        backend.flush(&mut graph_world, false);
        let state = piston_state_at(&backend, base);
        if native.piston_state().logical_tick < 3 {
            assert_eq!(state.availability, PistonAvailability::HeadPending);
            assert!(matches!(native.get_block(far), Block::MovingPiston { .. }));
            assert_eq!(piston_contact_block(&backend, far), Block::Air);
            assert_eq!(native.get_block(far_lamp), Block::RedstoneLamp { lit: false });
            assert_eq!(graph_world.get_block(far_lamp), Block::RedstoneLamp { lit: false });
        }
    }
    assert!(matches!(native.get_block(base), Block::Piston { piston } if piston.extended));
    assert!(matches!(native.get_block(base.offset(BlockFace::East)), Block::PistonHead { .. }));
    assert!(piston_state_at(&backend, base).extended);
    assert_eq!(piston_state_at(&backend, base).availability, PistonAvailability::Ready);
    assert!(matches!(
        piston_contact_block(&backend, base.offset(BlockFace::East)),
        Block::PistonHead { .. }
    ));
    assert_eq!(
        native.get_block(base.offset(BlockFace::East).offset(BlockFace::East)),
        piston_contact_block(&backend, base.offset(BlockFace::East).offset(BlockFace::East))
    );
    assert_eq!(native.get_block(far_lamp), Block::RedstoneLamp { lit: true });
    assert_eq!(graph_world.get_block(far_lamp), native.get_block(far_lamp));
}

#[test]
fn sticky_retraction_publishes_base_then_resource_once_in_native_order() {
    let (mut native, mut graph_world, base, lever_pos) = isolated_piston_pair(true);
    let near = base.offset(BlockFace::East);
    let far = near.offset(BlockFace::East);
    let watched = [base.offset(BlockFace::North), near.offset(BlockFace::North), far.offset(BlockFace::North)];
    for world in [&mut native, &mut graph_world] {
        for position in watched {
            world.set_block(
                position,
                Block::Observer {
                    observer: RedstoneObserver {
                        facing: BlockFacing::South,
                        powered: false,
                    },
                },
            );
        }
    }
    let mut backend = recording_graph(&graph_world, vec![]);

    set_native_lever_with_support(&mut native, lever_pos, true, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    for _ in 0..3 {
        tick_native_bounded(&mut native);
        backend.tick().unwrap();
    }
    assert!(matches!(native.get_block(base), Block::Piston { piston } if piston.extended));

    set_native_lever_with_support(&mut native, lever_pos, false, BlockFace::West);
    backend.use_block(lever_pos).unwrap();
    let mut final_callbacks = Vec::new();
    for tick in 1..=3 {
        let graph_start = backend.callback_order.len();
        let native_trace = capture_at(&watched, || tick_native_bounded(&mut native));
        backend.tick().unwrap();
        let native_callbacks = native_callbacks_at(&native_trace, &watched);
        assert_eq!(
            graph_callbacks_at(&backend, graph_start, &watched),
            native_callbacks,
            "retraction tick {tick}"
        );
        if tick == 3 {
            final_callbacks = native_callbacks;
        }
    }
    let base_callback = final_callbacks
        .iter()
        .position(|(position, _)| *position == watched[0])
        .expect("base completion should notify its observer");
    let resource_callback = final_callbacks
        .iter()
        .position(|(position, _)| *position == watched[1])
        .expect("resource completion should notify its observer");
    assert!(base_callback < resource_callback);
    assert!(matches!(native.get_block(base), Block::Piston { piston } if !piston.extended));
    assert!(!piston_state_at(&backend, base).extended);
    assert_eq!(piston_contact_block(&backend, near), Block::RedstoneBlock);
    assert!(backend.state.resources[0].enabled);
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
    let mut backend = recording_graph(&graph_world, vec![]);
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
        assert_eq!(native.pending_tick_at(far), backend.is_pending(observer_node));
        assert!(state.extended);
        if visit < 3 {
            assert_eq!(state.availability, PistonAvailability::HeadPending);
            assert!(matches!(native.get_block(base), Block::Piston { piston } if piston.extended));
        } else {
            assert_eq!(state.availability, PistonAvailability::Ready);
            assert!(matches!(native.get_block(base.offset(BlockFace::East)), Block::PistonHead { .. }));
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

    let result = GraphBackend::compile(&world, (BlockPos::new(0, 0, 0), BlockPos::new(10, 31, 15)), vec![], Arc::default());
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
fn external_wire_boundary_rejects_indirect_conductor_and_quasi_queries() {
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(10, 31, 15));
    let mut conductor_world = empty_world();
    let lamp = BlockPos::new(9, 20, 5);
    let conductor = BlockPos::new(10, 20, 5);
    let wire = BlockPos::new(11, 20, 5);
    let source = BlockPos::new(12, 20, 5);
    conductor_world.set_block(lamp, Block::RedstoneLamp { lit: false });
    conductor_world.set_block(conductor, Block::Stone {});
    conductor_world.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire {
                power: 15,
                ..RedstoneWire::default()
            },
        },
    );
    conductor_world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
    conductor_world.set_block(source, lever(true));
    conductor_world.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
    assert!(matches!(
        GraphBackend::compile(&conductor_world, bounds, vec![], Arc::default()),
        Err(GraphBackendError::UnsupportedComponent {
            kind: "external redstone wire boundary",
            ..
        })
    ));

    let mut quasi_world = empty_world();
    let base = BlockPos::new(9, 20, 5);
    quasi_world.set_block(
        base,
        Block::Piston {
            piston: mchprs_blocks::blocks::RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    let conductor = BlockPos::new(10, 21, 5);
    let wire = BlockPos::new(11, 21, 5);
    let source = BlockPos::new(12, 21, 5);
    quasi_world.set_block(conductor, Block::Stone {});
    quasi_world.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire {
                power: 15,
                ..RedstoneWire::default()
            },
        },
    );
    quasi_world.set_block(wire.offset(BlockFace::Bottom), Block::Stone {});
    quasi_world.set_block(source, lever(true));
    quasi_world.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
    assert!(matches!(
        GraphBackend::compile(&quasi_world, bounds, vec![], Arc::default()),
        Err(GraphBackendError::UnsupportedComponent {
            kind: "external redstone wire boundary",
            ..
        })
    ));
}

#[test]
fn stored_wire_diode_input_outside_selection_is_rejected() {
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(10, 31, 15));
    let repeater_pos = BlockPos::new(10, 20, 5);
    let wire_pos = repeater_pos.offset(BlockFace::East);
    let mut world = empty_world();
    world.set_block(
        repeater_pos,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::East,
                ..Default::default()
            },
        },
    );
    world.set_block(
        wire_pos,
        Block::RedstoneWire {
            wire: RedstoneWire {
                power: 15,
                ..RedstoneWire::default()
            },
        },
    );
    world.set_block(wire_pos.offset(BlockFace::Bottom), Block::Stone {});

    assert!(matches!(
        GraphBackend::compile(&world, bounds, vec![], Arc::default()),
        Err(GraphBackendError::UnsupportedComponent {
            kind: "external redstone wire boundary",
            ..
        })
    ));
}

#[test]
fn moved_redstone_resource_uses_its_selected_profile_strength() {
    let mut backend = contact_resource_backend();
    let target = SiteId::new(0);
    assert_eq!(backend.site_power(target, PowerPort::Weak(BlockFace::West)), 0);

    backend.state.resources[0].at = ContactId::new(1);

    assert_eq!(backend.site_power(target, PowerPort::Weak(BlockFace::West)), 15);
}

#[test]
fn fixed_boundary_comparator_keeps_its_sampled_analog_strength() {
    let backend = contact_resource_backend();
    assert_eq!(backend.site_power(SiteId::new(1), PowerPort::Output), 7);
    assert_eq!(backend.site_power(SiteId::new(1), PowerPort::Weak(BlockFace::South)), 7);
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
        turbo_callback_indices: vec![],
        wires: vec![],
        dust_paths: vec![],
        wire_relations: vec![],
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

#[test]
fn command_without_entity_survives_compile_reset_and_imported_tick() {
    let pos = BlockPos::new(8, 20, 5);
    for tick in [false, true] {
        let mut world = empty_world();
        world.set_block(pos, Block::from_name("command_block").unwrap());
        let ticks = if tick {
            vec![TickEntry {
                pos,
                ticks_left: 1,
                tick_priority: TickPriority::Normal,
                block_type: Some(world.get_block(pos).registry_id()),
            }]
        } else {
            vec![]
        };
        let mut backend = recording_graph(&world, ticks);
        if tick {
            backend.tick_with_world(&mut world).unwrap();
        }
        backend.reset(&mut world);
        assert!(world.get_block_entity(pos).is_none());
        assert!(world.command_output().next().is_none());
    }
}

#[test]
fn command_callback_observes_current_host_tick() {
    struct Host {
        tick: u64,
        calls: usize,
    }
    impl GraphCommandHost for Host {
        fn publish_logical_tick(&mut self, tick: u64) {
            self.tick = tick;
        }
        fn run_command(&mut self, _: &str, _: &str) -> Result<(), String> {
            assert_eq!(self.tick, 1);
            self.calls += 1;
            Ok(())
        }
        fn publish_command_entity(&mut self, _: BlockPos, _: BlockEntity) {}
    }
    let pos = BlockPos::new(8, 20, 5);
    let mut world = empty_world();
    world.set_block(pos, Block::from_name("command_block").unwrap());
    world.set_block_entity(
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say time".into(),
            automatic: true,
            ..Default::default()
        })),
    );
    let mut backend = recording_graph(&world, vec![]);
    let mut host = Host { tick: 0, calls: 0 };
    backend.tick_with_host(Some(&mut host)).unwrap();
    assert_eq!(host.calls, 1);
}

#[test]
fn compiler_returns_hostless_error_and_can_retry_with_world() {
    let pos = BlockPos::new(8, 20, 5);
    let mut world = empty_world();
    world.set_block(pos, Block::from_name("command_block").unwrap());
    world.set_block_entity(
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say retry".into(),
            automatic: true,
            ..Default::default()
        })),
    );
    let mut compiler = crate::redpiler::Compiler::default();
    compiler.compile(&world, BOUNDS, Default::default(), vec![], Default::default()).unwrap();
    for _ in 0..2 {
        assert_eq!(compiler.tick(), Err(GraphExecutionError::CommandHostUnavailable { position: pos }));
        assert_eq!(compiler.graph_execution_counts().unwrap().ticks, 0);
    }
    compiler.tick_with_world(&mut world).unwrap();
    assert_eq!(compiler.graph_execution_counts().unwrap().ticks, 1);
    assert_eq!(world.command_output().count(), 1);
}

#[test]
fn compiler_returns_latched_execution_failure_without_advancing_host() {
    let mut world = empty_world();
    world.set_block(LEVER_POS, lever(false));
    let mut compiler = crate::redpiler::Compiler::default();
    compiler.compile(&world, BOUNDS, Default::default(), vec![], Default::default()).unwrap();
    let error = GraphExecutionError::UnsupportedComponent {
        position: Some(LEVER_POS),
        kind: "injected execution failure",
    };
    let crate::redpiler::backend::Runtime::Graph(backend) = compiler.backend.as_mut().unwrap();
    backend.failure = Some(error.clone());
    for _ in 0..2 {
        assert_eq!(compiler.tick_with_world(&mut world), Err(error.clone()));
        assert_eq!(compiler.graph_execution_counts().unwrap().ticks, 0);
        assert_eq!(world.piston_state().logical_tick, 0);
    }
    compiler.reset(&mut world, BOUNDS);
    assert!(!compiler.is_active());
}
