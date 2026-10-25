//! Local wire transfers must preserve requests, full state, reset and native continuation.
use super::*;
use crate::redpiler::passes::planning::PlanBuilder;
use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};

fn prepare(world: &PlotWorld) -> passes::rich::PreparedGraph {
    passes::rich::prepare(world, BOUNDS, vec![], &TaskMonitor::default(), AnalysisLimits::default()).unwrap()
}

fn compile(world: &PlotWorld, enabled: bool) -> GraphBackend {
    let mut prepared = prepare(world);
    if enabled {
        let monitor = TaskMonitor::default();
        let options = Default::default();
        let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
        passes::compile_instant_wire_nodes::CompileInstantWireNodes
            .run(&mut prepared.graph, &context)
            .unwrap();
        assert_eq!(
            prepared.graph.wires.iter().filter(|wire| wire.execution.instant_node.is_some()).count(),
            1
        );
        let export = crate::redpiler::analysis::export_json::from_graph(&prepared.report, &prepared.graph).unwrap();
        let directory = root().join("target/instant-wire-nodes");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("private-cell.graph.json"), export.to_bytes().unwrap()).unwrap();
    }
    let (program, state) = prepared.graph.lower().unwrap();
    GraphBackend::new_unactivated(program, state, 0).unwrap()
}

fn same_future(general: &GraphBackend, candidate: &GraphBackend) {
    assert_eq!(candidate.state, general.state);
    assert_eq!(candidate.scheduled_ticks(), general.scheduled_ticks());
    assert_eq!(candidate.future.pending, general.future.pending);
    assert_eq!(
        candidate
            .future
            .piston_requests
            .iter()
            .map(|r| (r.piston, r.action, r.source))
            .collect::<Vec<_>>(),
        general
            .future
            .piston_requests
            .iter()
            .map(|r| (r.piston, r.action, r.source))
            .collect::<Vec<_>>()
    );
}

#[test]
fn instant_wire_node_matches_native_requests_reset_and_continuation() {
    for restore_before_acceptance in [false, true] {
        let mut world = wire_controlled_cell_world();
        let mut native = wire_controlled_cell_world();
        let mut general_world = wire_controlled_cell_world();
        let mut candidate = compile(&world, true);
        let mut general = compile(&world, false);
        let source = CONTROL.offset(BlockFace::West);
        let positions: Vec<_> = candidate.program.sites().iter().map(|site| site.position).collect();
        candidate.use_block(source).unwrap();
        general.use_block(source).unwrap();
        native_step(
            &mut native,
            Step::SetLever {
                position: source,
                powered: false,
            },
        );
        if restore_before_acceptance {
            candidate.use_block(source).unwrap();
            general.use_block(source).unwrap();
            native_step(
                &mut native,
                Step::SetLever {
                    position: source,
                    powered: true,
                },
            );
        }
        same_future(&general, &candidate);
        assert!(candidate.execution_counts().instant_wire_node_invocations > 0);
        assert_eq!(candidate.execution_counts().wire_walks, 0);
        assert!(candidate.execution_counts().callbacks < general.execution_counts().callbacks);
        for tick in 0..24 {
            candidate.tick().unwrap();
            general.tick().unwrap();
            native.tick_interpreted();
            same_future(&general, &candidate);
            check_boundary(
                &candidate,
                &native,
                &mut world,
                &positions,
                &format!("signal restore={restore_before_acceptance} tick={tick}"),
            )
            .unwrap();
            if tick == 0 {
                let mut handoff = wire_controlled_cell_world();
                candidate.flush(&mut handoff, false);
                candidate.project_future(&mut handoff);
                let mut expected = wire_controlled_cell_world();
                general.flush(&mut expected, false);
                general.project_future(&mut expected);
                for _ in 0..16 {
                    handoff.tick_interpreted();
                    expected.tick_interpreted();
                    for &position in &positions {
                        assert_eq!(handoff.get_block(position), expected.get_block(position));
                    }
                    assert_eq!(
                        handoff.scheduler().iter_entries().collect::<Vec<_>>(),
                        expected.scheduler().iter_entries().collect::<Vec<_>>()
                    );
                }
            }
        }
        candidate.reset(&mut world);
        general.reset(&mut general_world);
        for _ in 0..16 {
            world.tick_interpreted();
            general_world.tick_interpreted();
            native.tick_interpreted();
            for &position in &positions {
                assert_eq!(world.get_block(position), native.get_block(position));
                assert_eq!(general_world.get_block(position), native.get_block(position));
            }
            assert_eq!(
                world.scheduler().iter_entries().collect::<Vec<_>>(),
                native.scheduler().iter_entries().collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn instant_wire_node_declines_classic_consumers_and_other_dust() {
    for extra in [Block::RedstoneLamp { lit: false }, Block::RedstoneWire { wire: Default::default() }] {
        let mut world = wire_controlled_cell_world();
        world.set_block(CONTROL.offset(BlockFace::North), extra);
        let graph = prepare(&world).graph;
        let nodes = infer_instant_wire_nodes(&graph, &TaskMonitor::default()).unwrap();
        assert!(nodes.iter().all(Option::is_none));
    }
    let mut graph = prepare(&wire_controlled_cell_world()).graph;
    let mask = infer_instant_wire_nodes(&graph, &TaskMonitor::default()).unwrap()[0].unwrap();
    graph.wires[0].execution.instant_node = Some(InstantWireNode {
        receivers: mask.receivers ^ 1,
    });
    assert!(graph.validate().is_err(), "forged receiver closure must fail before execution");
}

fn add_cell(world: &mut PlotWorld, base: BlockPos, facing: BlockFacing) {
    let face: BlockFace = facing.into();
    world.set_block(
        base,
        Block::Piston {
            piston: RedstonePiston {
                facing,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        base.offset(face),
        Block::PistonHead {
            head: RedstonePistonHead {
                facing,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(base.offset(face).offset(face), Block::RedstoneBlock);
    world.set_block(
        base.offset(BlockFace::Top),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(base + BlockPos::new(0, 2, 0), Block::Stone {});
}

fn shared_bus_world(inhibited: bool) -> PlotWorld {
    let mut world = empty_world();
    for (base, facing) in [
        (BlockPos::new(11, 22, 12), BlockFacing::Down),
        (BlockPos::new(13, 22, 12), BlockFacing::Down),
        (BlockPos::new(12, 20, 11), BlockFacing::North),
    ] {
        add_cell(&mut world, base, facing);
    }
    for (control, powered) in [(BlockPos::new(12, 22, 12), true), (BlockPos::new(11, 23, 11), inhibited)] {
        world.set_block(control.offset(BlockFace::Top), Block::Stone {});
        world.set_block(
            control,
            Block::Lever {
                lever: Lever {
                    face: LeverFace::Ceiling,
                    facing: BlockDirection::North,
                    powered,
                },
            },
        );
    }
    let wire = BlockPos::new(12, 20, 12);
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

#[test]
fn instant_wire_node_shared_sources_and_inhibited_fanout_match_native() {
    for inhibited in [false, true] {
        let mut world = shared_bus_world(inhibited);
        let mut native = shared_bus_world(inhibited);
        let bounds = (BlockPos::new(2, 17, 2), BlockPos::new(20, 26, 20));
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
        let baseline = prepared.graph.clone();
        let options = Default::default();
        let context = passes::PassContext::new(&monitor, &options, &prepared.entry);
        passes::compile_instant_wire_nodes::CompileInstantWireNodes
            .run(&mut prepared.graph, &context)
            .unwrap();
        assert_eq!(
            prepared.graph.wires.iter().filter(|wire| wire.execution.instant_node.is_some()).count(),
            1
        );
        let (program, state) = prepared.graph.lower().unwrap();
        let mut candidate = GraphBackend::new_unactivated(program, state, 0).unwrap();
        let (program, state) = baseline.lower().unwrap();
        let mut general = GraphBackend::new_unactivated(program, state, 0).unwrap();
        let control = BlockPos::new(12, 22, 12);
        candidate.use_block(control).unwrap();
        general.use_block(control).unwrap();
        native_step(
            &mut native,
            Step::SetLever {
                position: control,
                powered: false,
            },
        );
        let positions: Vec<_> = candidate.program.sites().iter().map(|site| site.position).collect();
        for tick in 0..32 {
            candidate.tick().unwrap();
            general.tick().unwrap();
            native.tick_interpreted();
            same_future(&general, &candidate);
            check_boundary(
                &candidate,
                &native,
                &mut world,
                &positions,
                &format!("fanout inhibited={inhibited} tick={tick}"),
            )
            .unwrap();
        }
        if !inhibited {
            assert!(candidate.execution_counts().instant_wire_node_invocations > 0);
            assert!(candidate.execution_counts().wire_walks < general.execution_counts().wire_walks);
            assert!(candidate.execution_counts().piston_samples < general.execution_counts().piston_samples);
        } else {
            assert_eq!(
                candidate.execution_counts().instant_wire_node_invocations,
                0,
                "the retained source never changes the signal"
            );
        }
    }
}

#[test]
fn closed_multi_wire_boundary_accepts_instant_chain_and_rejects_classic_fanout() {
    let mut world = chain_world(true);
    for classic in [false, true] {
        if classic {
            world.set_block(BlockPos::new(12, 20, 9), Block::RedstoneLamp { lit: false });
        }
        let graph = prepare(&world).graph;
        let catalog = infer_boundaries(&graph, &TaskMonitor::default()).unwrap();
        let census = census_closed_instant_nets(&graph, &catalog, &TaskMonitor::default()).unwrap();
        assert_eq!(census.closed.iter().filter(|net| net.wires > 1).count(), usize::from(!classic));
    }
}

#[test]
fn closed_net_source_boundary_ignores_only_proven_false_inputs() {
    let mut world = chain_world(true);
    let torch = BlockPos::new(3, 20, 3);
    world.set_block(torch.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(torch, Block::RedstoneTorch { lit: true });
    let mut graph = prepare(&world).graph;
    let emitter = graph.nodes.iter().position(|node| node.position == Some(torch)).unwrap();
    let source = graph
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(NodeId::new(emitter as u32)))
        .unwrap();
    let always = GuardId::new(graph.guards.iter().position(|guard| *guard == Guard::Always).unwrap() as u32);
    let ordinal = graph.power_links.len();
    graph.power_links.push(PowerLink {
        from: SiteId::new(source as u32),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: graph.wires[0].node,
            port: InputPort::Main,
        },
        face: BlockFace::Top,
        attenuation: 0,
        guard: always,
    });
    let catalog = infer_boundaries(&graph, &TaskMonitor::default()).unwrap();
    let count = |graph: &CompileGraph| {
        census_closed_instant_nets(graph, &catalog, &TaskMonitor::default())
            .unwrap()
            .closed
            .iter()
            .filter(|net| net.wires > 1)
            .count()
    };
    assert_eq!(count(&graph), 0, "an external classic source is not inside the instant boundary");
    let never = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Not(always));
    graph.power_links[ordinal].guard = never;
    assert_eq!(count(&graph), 1, "a statically impossible source is not an effective connection");
    let variable = GuardId::new(graph.guards.len() as u32);
    graph
        .guards
        .push(Guard::State(StateTest::ResourceEnabled(catalog.cells[0].resources[0].resource)));
    graph.power_links[ordinal].guard = variable;
    assert_eq!(count(&graph), 0, "saved values cannot turn a variable source guard into a proof");
}
