use super::*;
use mchprs_blocks::{
    BlockPos,
    blocks::{RedstoneWire, RedstoneWireSide},
};

fn wire_graph(neighbors: &[Block; 24], wire: RedstoneWire) -> CompileGraph {
    CompileGraph {
        nodes: vec![CircuitNodeDraft {
            position: Some(BlockPos::new(20, 20, 20)),
            raw_state: Some(Block::RedstoneWire { wire }.get_id()),
            kind: CircuitNodeKind::Wire(0),
            state: ComponentState::Wire(wire),
            output_route: None,
        }],
        profiles: neighbors
            .iter()
            .map(|block| Profile {
                raw_state: block.get_id(),
                registry_type: 0,
                solid: block.is_solid(),
                transparent: block.is_transparent(),
                cube: block.is_cube(),
                electrical: ProfileElectrical::None,
            })
            .collect(),
        sites: (0..24)
            .map(|index| SiteSpec {
                position: BlockPos::new(index, 20, 20),
                source: SiteSource::Constant(ProfileId::new(index as u32)),
                source_strength: 0,
            })
            .collect(),
        wires: vec![WirePlan {
            node: NodeId::new(0),
            canonical_neighbors: std::array::from_fn(|index| SiteId::new(index as u32)),
            shape_route: None,
            turbo_route: None,
            execution: Default::default(),
        }],
        guards: vec![Guard::Always],
        links: vec![],
        power_links: vec![],
        comparator_inputs: vec![],
        contacts: vec![],
        routes: vec![],
        wire_relations: vec![],
        pistons: vec![],
        resources: vec![],
        initial_pistons: vec![],
        initial_resources: vec![],
        plans: ExecutionPlans::default(),
    }
}

#[test]
fn reset_wire_emission_matches_native_for_dots_crosses_and_all_local_shapes() {
    use crate::redstone::wire::{get_raw_sides_from_neighbors, regulate_sides};
    let cross = RedstoneWire::new(
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        15,
    );
    for saved in [
        cross,
        RedstoneWire::new(
            RedstoneWireSide::None,
            RedstoneWireSide::None,
            RedstoneWireSide::None,
            RedstoneWireSide::None,
            15,
        ),
    ] {
        for ceiling in [Block::Air, Block::Stone {}] {
            for mut shape in 0..6usize.pow(4) {
                let mut neighbors = [Block::Air; 24];
                neighbors[3] = ceiling;
                for (near, above, below) in [(4, 20, 17), (5, 21, 18), (1, 13, 12), (0, 8, 7)] {
                    match shape % 6 {
                        0 => {}
                        1 => neighbors[near] = Block::RedstoneWire { wire: cross },
                        2 => neighbors[near] = Block::Stone {},
                        3 => {
                            neighbors[near] = Block::Stone {};
                            neighbors[above] = Block::RedstoneWire { wire: cross };
                        }
                        4 => neighbors[below] = Block::RedstoneWire { wire: cross },
                        _ => neighbors[near] = Block::RedstoneBlock,
                    }
                    shape /= 6;
                }
                let graph = wire_graph(&neighbors, saved);
                let regulated = Block::RedstoneWire {
                    wire: regulate_sides(saved, get_raw_sides_from_neighbors(saved, &neighbors)),
                };
                for face in BlockFace::values() {
                    assert_eq!(
                        wire_emission::may_emit(&graph, NodeId::new(0), PowerPort::Weak(face)),
                        crate::redpiler::power_rules::emits_weak(regulated, face),
                        "{face:?}: {neighbors:?}"
                    );
                    assert_eq!(
                        wire_emission::may_emit(&graph, NodeId::new(0), PowerPort::Strong(face)),
                        crate::redpiler::power_rules::emits_strong(regulated, face)
                    );
                }
            }
        }
    }
}

#[test]
fn reset_search_bounds_attenuation_and_terminates_zero_loss_cycles() {
    let mut graph = wire_graph(&[Block::Air; 24], RedstoneWire::default());
    graph.nodes.push(CircuitNodeDraft {
        position: None,
        raw_state: None,
        kind: CircuitNodeKind::Observer {
            watched: SiteId::new(0),
            facing: BlockFace::Bottom,
        },
        state: ComponentState::Specialized,
        output_route: None,
    });
    for node in [NodeId::new(0), NodeId::new(1)] {
        graph.sites.push(SiteSpec {
            position: BlockPos::new(20, 20, 20),
            source: SiteSource::Component(node),
            source_strength: 0,
        });
    }
    let wire = PowerLink {
        from: SiteId::new(24),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: NodeId::new(2),
            port: InputPort::Quasi,
        },
        face: BlockFace::Top,
        attenuation: 0,
        guard: GuardId::new(0),
    };
    let source = PowerLink {
        from: SiteId::new(25),
        from_port: PowerPort::Strong(BlockFace::Bottom),
        to: PowerTarget::Input {
            node: NodeId::new(0),
            port: InputPort::Main,
        },
        attenuation: 14,
        ..wire
    };
    graph.power_links = vec![wire, PowerLink { to: source.to, ..wire }, source];
    let observers = FxHashMap::from_iter([(NodeId::new(1), PistonId::new(0))]);
    let monitor = TaskMonitor::default();
    let search = |graph: &CompileGraph| find(graph, &index_power(graph), &observers, NodeId::new(2), &monitor).unwrap();
    assert_eq!(search(&graph), Some((NodeId::new(1), PistonId::new(0))));
    graph.power_links[2].attenuation = 15;
    assert_eq!(search(&graph), None, "a strength-zero return is not a reset");
    graph.power_links[2].attenuation = 0;
    graph.guards.push(Guard::Not(GuardId::new(0)));
    graph.power_links[2].guard = GuardId::new(1);
    assert_eq!(search(&graph), None, "the zero-loss wire loop cannot make a false guard reachable");
    graph.power_links[2].guard = GuardId::new(0);
    graph.power_links[2].from_port = PowerPort::Strong(BlockFace::Top);
    assert_eq!(search(&graph), None, "the observer's dark face cannot close reset feedback");
    monitor.cancel();
    assert!(find(&graph, &index_power(&graph), &observers, NodeId::new(2), &monitor).is_err());
}

#[test]
fn reset_power_witness_does_not_borrow_an_independent_input_cone() {
    let mut graph = wire_graph(&[Block::Air; 24], RedstoneWire::default());
    let mut second = graph.nodes[0].clone();
    second.kind = CircuitNodeKind::Wire(1);
    graph.nodes.push(second);
    let mut second_plan = graph.wires[0].clone();
    second_plan.node = NodeId::new(1);
    graph.wires.push(second_plan);
    for node in [0, 1] {
        graph.sites.push(SiteSpec {
            position: BlockPos::new(node, 20, 20),
            source: SiteSource::Component(NodeId::new(node as u32)),
            source_strength: 0,
        });
    }
    let link = |source, node, port| PowerLink {
        from: SiteId::new(source),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: NodeId::new(node),
            port,
        },
        face: BlockFace::Top,
        attenuation: 0,
        guard: GuardId::new(0),
    };
    graph.power_links = vec![link(24, 2, InputPort::Quasi), link(25, 2, InputPort::Main), link(0, 0, InputPort::Main)];
    let mut witnessed = false;
    trace(&graph, &index_power(&graph), NodeId::new(2), &TaskMonitor::default(), |link, path| {
        if link.from == SiteId::new(0) {
            witnessed = true;
            assert!(path.contains(&PowerTarget::Input {
                node: NodeId::new(0),
                port: InputPort::Main
            }));
            assert!(
                !path.contains(&PowerTarget::Input {
                    node: NodeId::new(1),
                    port: InputPort::Main
                }),
                "an update on the other input cannot certify this reset source"
            );
        }
        None::<()>
    })
    .unwrap();
    assert!(witnessed);
}

#[test]
fn reset_search_traces_observer_through_furnace_component() {
    use mchprs_blocks::BlockDirection;
    let mut graph = wire_graph(&[Block::Air; 24], RedstoneWire::default());
    graph.nodes[0].raw_state = Some(
        Block::Furnace {
            facing: BlockDirection::North,
            lit: false,
        }
        .get_id(),
    );
    graph.nodes[0].kind = CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Constant);
    graph.nodes.push(CircuitNodeDraft {
        position: None,
        raw_state: None,
        kind: CircuitNodeKind::Observer {
            watched: SiteId::new(0),
            facing: BlockFace::Bottom,
        },
        state: ComponentState::Specialized,
        output_route: None,
    });
    for node in [NodeId::new(0), NodeId::new(1)] {
        graph.sites.push(SiteSpec {
            position: BlockPos::new(20, 20, 20),
            source: SiteSource::Component(node),
            source_strength: 0,
        });
    }
    graph.power_links = vec![
        PowerLink {
            from: SiteId::new(24),
            from_port: PowerPort::Conducted { include_dust: true },
            to: PowerTarget::Input {
                node: NodeId::new(2),
                port: InputPort::Quasi,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(0),
        },
        PowerLink {
            from: SiteId::new(25),
            from_port: PowerPort::Strong(BlockFace::Bottom),
            to: PowerTarget::Conductor {
                site: SiteId::new(24),
                include_dust: true,
            },
            face: BlockFace::Bottom,
            attenuation: 0,
            guard: GuardId::new(0),
        },
    ];
    let observers = FxHashMap::from_iter([(NodeId::new(1), PistonId::new(0))]);
    let monitor = TaskMonitor::default();
    let search = |graph: &CompileGraph| find(graph, &index_power(graph), &observers, NodeId::new(2), &monitor).unwrap();
    assert_eq!(search(&graph), Some((NodeId::new(1), PistonId::new(0))));
    graph.nodes[0].raw_state = Some(
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        }
        .get_id(),
    );
    assert_eq!(search(&graph), None, "a nonconducting component cannot relay reset power");
    graph.nodes[0].raw_state = None;
    assert_eq!(search(&graph), None, "a component with unknown geometry cannot relay reset power");
    graph.nodes[0].raw_state = Some(
        Block::Furnace {
            facing: BlockDirection::North,
            lit: false,
        }
        .get_id(),
    );
    graph.guards.push(Guard::Not(GuardId::new(0)));
    graph.power_links[1].guard = GuardId::new(1);
    assert_eq!(search(&graph), None, "a false source guard cannot relay reset power");
}
