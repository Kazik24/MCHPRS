//! Runtime query certificates must preserve live and captured wire operands.
use super::*;
use crate::plot::PlotWorld;
use crate::world::storage::Chunk;
use mchprs_blocks::BlockDirection;
use mchprs_blocks::blocks::RedstoneComparator;

const ROOT: BlockPos = BlockPos::new(8, 20, 8);
const NEIGHBOR: BlockPos = BlockPos::new(9, 20, 8);
const DIRECT: BlockPos = BlockPos::new(7, 20, 8);
const CONDUCTOR: BlockPos = BlockPos::new(8, 20, 9);
const STRONG: BlockPos = BlockPos::new(8, 20, 10);

fn query_graph() -> CompileGraph {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for position in [ROOT, NEIGHBOR] {
        world.set_block(position, Block::RedstoneWire { wire: Default::default() });
    }
    for (position, facing) in [(DIRECT, BlockDirection::West), (STRONG, BlockDirection::South)] {
        world.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(facing, ComparatorMode::Compare, false),
            },
        );
        world.set_block_entity(position, BlockEntity::Comparator { output_strength: 0 });
    }
    for position in [ROOT, NEIGHBOR, DIRECT, STRONG] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    world.set_block(CONDUCTOR, Block::Stone {});
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

fn backend(mut graph: CompileGraph) -> GraphBackend {
    for index in 0..graph.wires.len() {
        graph.wires[index].execution.query_facts = Some(WireQueryFacts::infer(&graph, &graph.wires[index]));
    }
    let (program, state) = graph.lower().unwrap();
    GraphBackend::new(program, state, 0).unwrap()
}

fn root_plan(backend: &GraphBackend) -> WirePlan {
    backend.wire_plan(backend.positions[&ROOT]).clone()
}

fn set_strength(backend: &mut GraphBackend, position: BlockPos, strength: u8) {
    let node = backend.positions[&position];
    match &mut backend.state.components[node.index()] {
        ComponentState::Electrical(state) => {
            state.powered = strength != 0;
            state.output_strength = strength;
        }
        ComponentState::Wire(wire) => wire.power = strength,
        ComponentState::Specialized => panic!("query source must own electrical or wire state"),
    }
}

fn capture_neighbors(backend: &GraphBackend, plan: &WirePlan) -> WireWalk {
    let mut walk = WireWalk::new();
    let root = backend.component_sites[plan.node.index()].unwrap();
    for site in plan.canonical_neighbors.into_iter().chain([root]) {
        walk.insert_site(
            site,
            WalkSite {
                site,
                observed: backend.resolve_site(site, None).unwrap().observed(),
                visited: site == root,
                x_bias: 0,
                z_bias: 0,
                layer: 0,
                heading: None,
                neighbor_slots: None,
            },
        );
    }
    walk
}

fn assert_seed_matches(backend: &mut GraphBackend, plan: &WirePlan) -> u8 {
    let mut general = plan.clone();
    general.execution.query_facts = None;
    let expected = backend.seed_power_general(&general);
    let actual = backend.seed_power(plan);
    assert_eq!(actual, expected, "live seed query");
    actual.0
}

fn assert_captured_matches(backend: &mut GraphBackend, plan: &WirePlan, walk: &mut WireWalk) -> u8 {
    let ordered = backend.identify_wire_neighbors(plan.node, walk);
    let mut general = plan.clone();
    general.execution.query_facts = None;
    let expected = backend.turbo_wire_power_general(&general, &ordered, walk);
    let actual = backend.turbo_wire_power(plan, &ordered, walk);
    assert_eq!(actual, expected, "captured Turbo query");
    actual.0
}

fn profile(graph: &mut CompileGraph, block: Block, solid: bool, transparent: bool) -> ProfileId {
    let id = ProfileId::new(graph.profiles.len() as u32);
    graph.profiles.push(Profile {
        raw_state: block.get_id(),
        registry_type: block.registry_id(),
        solid,
        transparent,
        cube: block.is_cube(),
        electrical: ProfileElectrical::None,
    });
    id
}

fn set_constant_neighbor(graph: &mut CompileGraph, slot: usize, profile: ProfileId) {
    let root = graph
        .wires
        .iter()
        .find(|wire| graph.nodes[wire.node.index()].position == Some(ROOT))
        .unwrap();
    let site = root.canonical_neighbors[slot];
    graph.sites[site.index()].source = SiteSource::Constant(profile);
}

#[test]
fn certificates_preserve_every_live_input_and_adjacent_wire_strength() {
    let mut backend = backend(query_graph());
    let plan = root_plan(&backend);
    assert!(plan.execution.query_facts.is_some());
    for input in 0..=15 {
        for wire in 0..=15 {
            set_strength(&mut backend, DIRECT, input);
            set_strength(&mut backend, NEIGHBOR, wire);
            let expected = input.max(wire.saturating_sub(1));
            assert_eq!(assert_seed_matches(&mut backend, &plan), expected);
            let mut walk = capture_neighbors(&backend, &plan);
            assert_eq!(assert_captured_matches(&mut backend, &plan, &mut walk), expected);
        }
    }
}

#[test]
fn captured_wire_strength_stays_old_while_conducted_strong_power_stays_live() {
    let mut backend = backend(query_graph());
    let plan = root_plan(&backend);
    assert_ne!(plan.execution.query_facts.unwrap().solid & (1 << 5), 0);
    set_strength(&mut backend, NEIGHBOR, 13);
    let mut walk = capture_neighbors(&backend, &plan);
    set_strength(&mut backend, NEIGHBOR, 1);
    for strength in 0..=15 {
        set_strength(&mut backend, STRONG, strength);
        assert_eq!(assert_seed_matches(&mut backend, &plan), strength);
        assert_eq!(assert_captured_matches(&mut backend, &plan, &mut walk), strength.max(12));
    }
}

#[test]
fn profile_geometry_flags_control_steps_even_when_raw_stone_disagrees() {
    for (solid, transparent, below, above, expected) in [(false, true, 15, 0, 14), (true, true, 0, 15, 0), (true, false, 0, 15, 14)] {
        let mut graph = query_graph();
        let neighbor = profile(&mut graph, Block::Stone {}, solid, transparent);
        set_constant_neighbor(&mut graph, 4, neighbor);
        for (slot, power) in [(17, below), (20, above)] {
            let wire = Block::RedstoneWire {
                wire: RedstoneWire { power, ..Default::default() },
            };
            let wire_profile = profile(&mut graph, wire, false, wire.is_transparent());
            set_constant_neighbor(&mut graph, slot, wire_profile);
        }
        let mut backend = backend(graph);
        let plan = root_plan(&backend);
        assert_eq!(assert_seed_matches(&mut backend, &plan), expected);
        let mut walk = capture_neighbors(&backend, &plan);
        assert_eq!(assert_captured_matches(&mut backend, &plan, &mut walk), expected);
    }
}

#[test]
fn dynamic_contact_geometry_uses_live_seed_and_captured_turbo_selection() {
    let mut graph = query_graph();
    let stone = profile(&mut graph, Block::Stone {}, true, false);
    let air = profile(&mut graph, Block::Air, false, true);
    for (slot, power) in [(17, 11), (20, 15)] {
        let wire = Block::RedstoneWire {
            wire: RedstoneWire { power, ..Default::default() },
        };
        let wire_profile = profile(&mut graph, wire, false, wire.is_transparent());
        set_constant_neighbor(&mut graph, slot, wire_profile);
    }
    let node = NodeId::new(graph.nodes.len() as u32);
    let contact = ContactId::new(0);
    let resource = ResourceId::new(0);
    let enabled = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(StateTest::ResourceEnabled(resource)));
    let disabled = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Not(enabled));
    let position = ROOT.offset(BlockFace::North);
    graph.nodes.push(CircuitNodeDraft {
        position: Some(position),
        raw_state: None,
        kind: CircuitNodeKind::Contact(contact),
        state: ComponentState::Specialized,
        output_route: None,
    });
    graph.contacts.push(ContactSpec {
        node,
        position,
        variants: vec![
            ContactVariant {
                when: enabled,
                profile: stone,
                receiver: None,
            },
            ContactVariant {
                when: disabled,
                profile: air,
                receiver: None,
            },
        ]
        .into_boxed_slice(),
    });
    graph.resources.push(ResourceSpec {
        material: stone,
        contacts: vec![contact].into_boxed_slice(),
    });
    graph.initial_resources.push(ResourceState {
        at: contact,
        enabled: true,
        revision: 0,
    });
    let site = graph.sites.iter_mut().find(|site| site.position == position).unwrap();
    site.source = SiteSource::Contact(contact);
    let mut backend = backend(graph);
    let plan = root_plan(&backend);
    assert_eq!(plan.execution.query_facts.unwrap().known & (1 << 4), 0);
    let mut walk = capture_neighbors(&backend, &plan);
    assert_eq!(assert_seed_matches(&mut backend, &plan), 14);
    backend.state.resources[0].enabled = false;
    assert_eq!(assert_seed_matches(&mut backend, &plan), 10);
    assert_eq!(assert_captured_matches(&mut backend, &plan, &mut walk), 14);
    let mut fresh_walk = capture_neighbors(&backend, &plan);
    assert_eq!(assert_captured_matches(&mut backend, &plan, &mut fresh_walk), 10);
}

#[test]
fn typed_wire_class_overrides_mismatched_raw_state_in_query_certificates() {
    let mut graph = query_graph();
    let node = graph.nodes.iter_mut().find(|node| node.position == Some(NEIGHBOR)).unwrap();
    assert!(matches!(node.kind, CircuitNodeKind::Wire(_)));
    node.raw_state = Some(Block::Stone {}.get_id());
    let mut backend = backend(graph);
    let plan = root_plan(&backend);
    set_strength(&mut backend, NEIGHBOR, 15);
    assert_eq!(assert_seed_matches(&mut backend, &plan), 14);
    let mut walk = capture_neighbors(&backend, &plan);
    assert_eq!(assert_captured_matches(&mut backend, &plan, &mut walk), 14);
}

#[test]
fn changing_a_compiled_input_mask_invalidates_its_certificate() {
    let mut graph = query_graph();
    let wire = graph
        .wires
        .iter()
        .position(|wire| graph.nodes[wire.node.index()].position == Some(ROOT))
        .unwrap();
    let mut facts = WireQueryFacts::infer(&graph, &graph.wires[wire]);
    assert_ne!(facts.transfer.unwrap().seed.direct, 0);
    facts.transfer.as_mut().unwrap().seed.direct = 0;
    graph.wires[wire].execution.query_facts = Some(facts);
    assert!(matches!(graph.validate(), Err(GraphBuildError::InvalidWireQueryFacts { .. })));
}

#[test]
fn seed_and_turbo_keep_their_distinct_vertical_and_ascent_rules() {
    for diagonal in [false, true] {
        let mut graph = query_graph();
        let wire = Block::RedstoneWire {
            wire: RedstoneWire {
                power: 15,
                ..Default::default()
            },
        };
        let source = profile(&mut graph, wire, false, wire.is_transparent());
        if diagonal {
            let opaque_nonsolid = profile(&mut graph, Block::Stone {}, false, false);
            set_constant_neighbor(&mut graph, 4, opaque_nonsolid);
            set_constant_neighbor(&mut graph, 20, source);
        } else {
            set_constant_neighbor(&mut graph, 3, source);
        }
        let mut backend = backend(graph);
        let plan = root_plan(&backend);
        assert_eq!(assert_seed_matches(&mut backend, &plan), 14);
        let mut walk = capture_neighbors(&backend, &plan);
        assert_eq!(assert_captured_matches(&mut backend, &plan, &mut walk), 0);
    }
}
