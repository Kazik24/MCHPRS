//! Boundary state, future work and projected dust, independently checked against native.
use super::*;
use crate::redpiler::passes::{PassContext, planning::PlanBuilder};

pub(super) fn local_net_pipeline(options: &crate::redpiler::CompilerOptions) -> passes::PassPipeline {
    let mut pipeline = passes::default_graph_pipeline(options);
    pipeline
        .entries
        .retain(|entry| !matches!(&entry.pass, passes::Pass::Build(builder) if builder.name() == "compile-fixed-classic"));
    pipeline
}

fn line(length: i32) -> PlotWorld {
    let mut world = fixture_world();
    world.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(LEVER_POS, lever(false));
    for x in 6..6 + length {
        let position = BlockPos::new(x, 20, 5);
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(position, Block::RedstoneWire { wire: Default::default() });
    }
    let position = BlockPos::new(6 + length, 20, 5);
    world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        position,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::West,
                delay: 2,
                powered: false,
                locked: false,
            },
        },
    );
    for x in 6..6 + length {
        let position = BlockPos::new(x, 20, 5);
        let wire = redstone::wire::get_state_for_placement(&world, position);
        world.set_block(position, Block::RedstoneWire { wire });
        redstone::update(world.get_block(position), &mut world, position, None);
    }
    world
}

fn prepare(world: &PlotWorld) -> passes::rich::PreparedGraph {
    passes::rich::prepare(
        world,
        (BlockPos::new(0, 0, 0), BlockPos::new(31, 31, 31)),
        vec![],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
}

fn optimize(graph: &mut CompileGraph, entry: &EntryState) {
    let monitor = TaskMonitor::default();
    passes::compile_classic_redstone::CompileClassicRedstone
        .run(graph, &PassContext::new(&monitor, &Default::default(), entry))
        .unwrap();
    if graph.plans.classic_nets.is_empty() {
        println!("classic declined: {:?}", monitor.graph_statistics().preparation_tables);
    }
}

fn backend(graph: CompileGraph) -> GraphBackend {
    let (program, state) = graph.lower().unwrap();
    GraphBackend::new(program, state, 0).unwrap()
}

fn assert_same(general: &GraphBackend, classic: &GraphBackend) {
    assert_eq!(general.scheduled_ticks(), classic.scheduled_ticks(), "deadline, priority and FIFO");
    for index in 0..general.program.nodes().len() {
        let node = NodeId::new(index as u32);
        assert_eq!(general.node_block(node), classic.node_block(node), "projected node {node:?}");
    }
    assert_eq!(general.future.pending, classic.future.pending);
}

#[test]
fn classic_line_pulses_preserve_future_and_never_write_private_dust() {
    for length in [1, 2, 4, 8, 16] {
        let mut native = line(length);
        let mut prepared = prepare(&native);
        let mut general = backend(prepared.graph.clone());
        optimize(&mut prepared.graph, &prepared.entry);
        assert!(!prepared.graph.plans.classic_nets.is_empty(), "length {length}");
        prepared.graph.validate().unwrap();
        let mut classic = backend(prepared.graph);
        let initial = classic.state.components.clone();
        for (step, ticks) in [0, 1, 0, 1, 2, 4, 1, 8].into_iter().enumerate() {
            for graph in [&mut general, &mut classic] {
                graph.use_block(LEVER_POS).unwrap();
            }
            set_native_lever(&mut native, LEVER_POS, step % 2 == 0);
            assert_same(&general, &classic);
            assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
            for _ in 0..ticks {
                general.tick().unwrap();
                classic.tick().unwrap();
                native.tick_interpreted();
                assert_same(&general, &classic);
                assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
                for (&position, &node) in &classic.positions {
                    assert_eq!(classic.node_block(node), native.get_block(position));
                }
            }
        }
        assert!(general.counts.wire_walks > 0);
        assert_eq!(classic.counts.wire_walks, 0);
        assert!(classic.counts.classic_net_invocations > 0);
        for wire in &classic.program.classic().wires {
            assert_eq!(classic.state.components[wire.node.index()], initial[wire.node.index()]);
        }
        let mut resumed = line(length);
        classic.reset(&mut resumed);
        for _ in 0..8 {
            resumed.tick_interpreted();
            native.tick_interpreted();
            for &position in general.positions.keys() {
                assert_eq!(resumed.get_block(position), native.get_block(position));
            }
        }
    }
}

#[test]
fn classic_signal_commits_at_each_original_occurrence_and_certificate_is_checked() {
    let world = line(4);
    let mut prepared = prepare(&world);
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
    let plan = prepared.graph.plans.classic_nets[0].clone();
    let mut corrupt = prepared.graph.clone();
    corrupt.plans.classic_nets[0].wires[0].attenuation = 14;
    assert!(corrupt.validate().is_err());
    let mut corrupt = prepared.graph.clone();
    corrupt.plans.classic_nets[0]
        .entries
        .iter_mut()
        .find(|entry| !entry.ops.is_empty())
        .unwrap()
        .ops[0]
        .when[0] ^= 1;
    assert!(corrupt.validate().is_err());
    let mut classic = backend(prepared.graph);
    let source = plan.source;
    let ComponentState::Electrical(state) = &mut classic.state.components[source.index()] else {
        unreachable!()
    };
    state.powered = true;
    state.output_strength = 15;
    let wire = plan.wires[0].node;
    assert_eq!(classic.wire_state(wire).power, 0, "new output cannot publish before route entry");
    let entry = classic.program.classic().entries.iter().position(|entry| entry.commit).unwrap();
    classic.run_classic_entry(entry);
    assert_eq!(classic.wire_state(wire).power, 15);
    let future = classic.scheduled_ticks();
    classic.run_classic_entry(entry);
    assert_eq!(classic.scheduled_ticks(), future, "duplicate occurrence preserves pending membership");
}

#[test]
fn classic_declines_stale_strength_and_imported_private_wire_work() {
    let mut world = line(2);
    let position = NEXT_POS;
    let Block::RedstoneWire { mut wire } = world.get_block(position) else {
        unreachable!()
    };
    wire.power = 9;
    world.set_block(position, Block::RedstoneWire { wire });
    let mut prepared = prepare(&world);
    optimize(&mut prepared.graph, &prepared.entry);
    assert!(prepared.graph.plans.classic_nets.is_empty());
    let mut prepared = prepare(&line(2));
    let tick = TickEntry {
        pos: NEXT_POS,
        block_type: None,
        ticks_left: 1,
        tick_priority: TickPriority::Normal,
    };
    prepared.entry.pending_ticks.push(EntryTick {
        tick,
        current_registry_type: Some(Block::RedstoneWire { wire: Default::default() }.registry_id()),
    });
    optimize(&mut prepared.graph, &prepared.entry);
    assert!(prepared.graph.plans.classic_nets.is_empty());
}

#[test]
fn classic_bent_branch_keeps_native_receiver_fifo_and_exported_ownership() {
    let mut native = fixture_world();
    native.set_block(LEVER_POS, lever(false));
    native.set_block(LEVER_POS.offset(BlockFace::Bottom), Block::Stone {});
    let points = [(6, 5), (7, 5), (7, 6), (7, 7), (8, 6), (9, 6)];
    for (x, z) in points {
        let position = BlockPos::new(x, 20, z);
        native.set_block(position, Block::RedstoneWire { wire: Default::default() });
        native.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    for (x, z, facing) in [(7, 8, BlockDirection::North), (10, 6, BlockDirection::West)] {
        let position = BlockPos::new(x, 20, z);
        native.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        native.set_block(
            position,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater {
                    facing,
                    delay: 2,
                    powered: false,
                    locked: false,
                },
            },
        );
    }
    for (x, z) in points {
        let position = BlockPos::new(x, 20, z);
        let wire = redstone::wire::get_state_for_placement(&native, position);
        native.set_block(position, Block::RedstoneWire { wire });
    }
    let mut prepared = prepare(&native);
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
    assert_eq!(prepared.graph.plans.classic_nets[0].inputs.len(), 2);
    let export = crate::redpiler::analysis::export_json::from_graph(&prepared.report, &prepared.graph).unwrap();
    let json: Value = serde_json::from_slice(&export.to_bytes().unwrap()).unwrap();
    assert_eq!(json["graph"]["classic_nets"].as_array().unwrap().len(), 1);
    assert!(
        json["graph"]["routes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|route| route["deliveries"].as_array().unwrap())
            .any(|delivery| delivery["execution"] == "classic_provenance")
    );
    let mut classic = backend(prepared.graph);
    for step in 0..24 {
        if step % 3 == 0 {
            general.use_block(LEVER_POS).unwrap();
            classic.use_block(LEVER_POS).unwrap();
            set_native_lever(&mut native, LEVER_POS, (step / 3) % 2 == 0);
        }
        assert_same(&general, &classic);
        assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
        general.tick().unwrap();
        classic.tick().unwrap();
        native.tick_interpreted();
    }
    assert_eq!(classic.counts.wire_walks, 0);
}

fn real_fixture() -> (PlotWorld, (BlockPos, BlockPos)) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/graph-fixtures/simple_and_gate.schem");
    let mut clipboard = load_schematic(std::fs::File::open(path).unwrap()).unwrap();
    clipboard.offset_x = 0;
    clipboard.offset_y = 0;
    clipboard.offset_z = 0;
    let origin = BlockPos::new(8, 8, 8);
    let bounds = (
        origin,
        origin + BlockPos::new(clipboard.size_x as i32 - 1, clipboard.size_y as i32 - 1, clipboard.size_z as i32 - 1),
    );
    let mut world = fixture_world();
    paste_clipboard(&mut world, &clipboard, origin, false);
    (world, bounds)
}

#[test]
#[ignore = "five paired active real-fixture samples; run release for performance evidence"]
fn classic_real_fixture_paired_release() {
    let (world, bounds) = real_fixture();
    let monitor = TaskMonitor::default();
    let mut general_draft = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let mut classic_draft = general_draft.graph.clone();
    let mut compile_times = Vec::new();
    for (graph, enabled) in [(&mut general_draft.graph, false), (&mut classic_draft, true)] {
        let options = crate::redpiler::CompilerOptions {
            optimization_level: Some(2),
            classic_redstone: enabled,
            ..Default::default()
        };
        let started = std::time::Instant::now();
        passes::run_graph_pipeline(graph, &options, &general_draft.entry, &local_net_pipeline(&options), &monitor).unwrap();
        compile_times.push(started.elapsed().as_secs_f64());
    }
    assert!(!classic_draft.plans.classic_nets.is_empty(), "real positive fixture must be admitted");
    let export = crate::redpiler::analysis::export_json::from_graph(&general_draft.report, &classic_draft).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::write(
        root.join("test_data/graph-fixtures/simple_and_gate-classic.graph.json"),
        export.to_bytes().unwrap(),
    )
    .unwrap();
    let controls = general_draft
        .graph
        .nodes
        .iter()
        .filter(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Lever)))
        .map(|node| node.position.unwrap())
        .collect::<Vec<_>>();
    assert!(!controls.is_empty());
    let mut rows = Vec::new();
    for sample in 0..5 {
        let mut general = backend(general_draft.graph.clone());
        let mut classic = backend(classic_draft.clone());
        let (mut native, _) = real_fixture();
        let run_graph = |backend: &mut GraphBackend| {
            let started = std::time::Instant::now();
            for event in 0..2000 {
                backend.use_block(controls[event % controls.len()]).unwrap();
                backend.tick().unwrap();
                backend.tick().unwrap();
            }
            started.elapsed().as_secs_f64()
        };
        let (general_time, classic_time) = if sample % 2 == 0 {
            (run_graph(&mut general), run_graph(&mut classic))
        } else {
            let classic_time = run_graph(&mut classic);
            (run_graph(&mut general), classic_time)
        };
        let started = std::time::Instant::now();
        for event in 0..2000 {
            let position = controls[event % controls.len()];
            let Block::Lever { lever } = native.get_block(position) else {
                unreachable!()
            };
            set_native_lever_as_interaction(&mut native, position, !lever.powered);
            native.tick_interpreted();
            native.tick_interpreted();
        }
        let native_time = started.elapsed().as_secs_f64();
        assert_same(&general, &classic);
        assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
        for (&position, &node) in &classic.positions {
            assert_eq!(classic.node_block(node), native.get_block(position));
        }
        let row = serde_json::json!({"sample":sample,"general_seconds":general_time,"classic_seconds":classic_time,"interpreter_seconds":native_time,"general_walks":general.counts.wire_walks,"classic_walks":classic.counts.wire_walks,"general_callbacks":general.counts.callbacks,"classic_callbacks":classic.counts.callbacks,"classic_invocations":classic.counts.classic_net_invocations,"classic_ops":classic.counts.classic_receiver_ops,"classic_program_bytes":classic.program.classic().estimated_bytes(),"classic_state_bytes":classic.state.classic_signals.len()});
        println!("CLASSIC BENCH {row}");
        rows.push(row);
    }
    std::fs::write(root.join("target/classic-real-fixture-benchmark.json"),serde_json::to_vec_pretty(&serde_json::json!({"fixture":"test_data/graph-fixtures/simple_and_gate.schem","profile":if cfg!(debug_assertions){"debug"}else{"release"},"events_per_sample":2000,"ticks_per_event":2,"pipeline_seconds":compile_times,"samples":rows})).unwrap()).unwrap();
}

#[test]
fn classic_delayed_torch_feedback_keeps_each_pending_deadline() {
    let mut native = fixture_world();
    let source = LEVER_POS;
    native.set_block(
        source,
        Block::RedstoneWallTorch {
            lit: false,
            facing: BlockDirection::North,
        },
    );
    native.set_block(source.offset(BlockFace::South), Block::Stone {});
    let points = [(6, 5), (7, 5), (7, 4), (6, 4), (5, 4), (4, 4), (3, 4), (3, 5), (3, 6), (4, 6)];
    for (x, z) in points {
        let position = BlockPos::new(x, 20, z);
        native.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        native.set_block(position, Block::RedstoneWire { wire: Default::default() });
    }
    for (x, z) in points {
        let position = BlockPos::new(x, 20, z);
        let wire = redstone::wire::get_state_for_placement(&native, position);
        native.set_block(position, Block::RedstoneWire { wire });
    }
    let mut prepared = prepare(&native);
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1, "delayed cycle is admitted structurally");
    let mut classic = backend(prepared.graph);
    for graph in [&mut general, &mut classic] {
        let node = graph.positions[&source];
        graph.update_component(node, DeliveryKind::Update { direction: None }, None).unwrap();
    }
    redstone::update(native.get_block(source), &mut native, source, None);
    let mut transitions = 0;
    let mut previous = false;
    for _ in 0..32 {
        assert_same(&general, &classic);
        assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
        general.tick().unwrap();
        classic.tick().unwrap();
        native.tick_interpreted();
        let powered = classic.node_state(classic.positions[&source]).powered;
        transitions += usize::from(powered != previous);
        previous = powered;
        for (&position, &node) in &classic.positions {
            assert_eq!(classic.node_block(node), native.get_block(position));
        }
    }
    assert!(transitions >= 4, "feedback must stay active");
    assert_eq!(classic.counts.wire_walks, 0);
}

#[test]
fn classic_declines_effectful_and_independently_variable_boundaries_but_not_remote_pistons() {
    for variant in 0..4 {
        let mut world = line(4);
        match variant {
            0 => {
                world.set_block(
                    BlockPos::new(7, 20, 6),
                    Block::Observer {
                        observer: RedstoneObserver {
                            facing: BlockFace::North.into(),
                            powered: false,
                        },
                    },
                );
            }
            1 => {
                world.set_block(
                    BlockPos::new(10, 20, 5),
                    Block::RedstoneComparator {
                        comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
                    },
                );
            }
            2 => {
                world.set_block(
                    BlockPos::new(10, 20, 6),
                    Block::RedstoneRepeater {
                        repeater: RedstoneRepeater {
                            facing: BlockDirection::South,
                            ..Default::default()
                        },
                    },
                );
            }
            _ => {
                let position = BlockPos::new(7, 20, 7);
                world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
                world.set_block(position, Block::RedstoneWire { wire: Default::default() });
            }
        }
        let mut prepared = prepare(&world);
        optimize(&mut prepared.graph, &prepared.entry);
        if variant == 1 {
            assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
        } else if variant == 3 {
            assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
            let guarded = &prepared.graph.plans.classic_nets[0].guarded_wires;
            assert_eq!(guarded.len(), 1);
            assert_eq!(prepared.graph.nodes[guarded[0].index()].position, Some(BlockPos::new(7, 20, 7)));
        } else {
            assert!(prepared.graph.plans.classic_nets.is_empty(), "variant {variant}");
        }
        prepared.graph.validate().unwrap();
    }
    let mut world = line(4);
    world.set_block(
        BlockPos::new(25, 20, 25),
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFace::East.into(),
                sticky: false,
                extended: false,
            },
        },
    );
    let mut prepared = prepare(&world);
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1, "remote piston must not reject local net");
    let mut overlap = prepared.graph.clone();
    overlap.plans.classic_nets.push(overlap.plans.classic_nets[0].clone());
    assert!(overlap.validate().is_err());
}

#[test]
fn classic_live_ingress_and_inert_shape_do_not_publish_dust_early() {
    let mut world = fixture_world();
    let source_position = BlockPos::new(5, 19, 5);
    let wire_position = BlockPos::new(6, 20, 5);
    let receiver_position = BlockPos::new(6, 19, 6);
    world.set_block(BlockPos::new(6, 19, 5), Block::Stone {});
    world.set_block(
        source_position,
        Block::Lever {
            lever: Lever::new(LeverFace::Wall, BlockDirection::West, false),
        },
    );
    world.set_block(
        receiver_position,
        Block::RedstoneWallTorch {
            lit: true,
            facing: BlockDirection::South,
        },
    );
    world.set_block(wire_position, Block::RedstoneWire { wire: Default::default() });
    let wire = redstone::wire::get_state_for_placement(&world, wire_position);
    world.set_block(wire_position, Block::RedstoneWire { wire });
    let mut prepared = prepare(&world);
    let source = prepared
        .graph
        .nodes
        .iter()
        .position(|node| node.position == Some(source_position))
        .map(|node| NodeId::new(node as u32))
        .unwrap();
    let wire = prepared
        .graph
        .nodes
        .iter()
        .position(|node| node.position == Some(wire_position))
        .map(|node| NodeId::new(node as u32))
        .unwrap();
    let site = prepared
        .graph
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(wire))
        .map(|site| SiteId::new(site as u32))
        .unwrap();
    let route = prepared.graph.nodes[source.index()].output_route.unwrap();
    let sequence = prepared.graph.routes[route.index()]
        .deliveries
        .last()
        .map_or(0, |delivery| delivery.sequence + 1);
    let guard = prepared
        .graph
        .guards
        .iter()
        .position(|guard| *guard == Guard::Always)
        .map(|guard| GuardId::new(guard as u32))
        .unwrap();
    let delivery = RouteDelivery {
        target: site,
        kind: DeliveryKind::Shape {
            direction: BlockFace::Bottom,
        },
        guard,
        view: OperandView::Live,
        sequence,
        receiver: Some(CallbackReceiver::Component(wire)),
    };
    let mut deliveries = prepared.graph.routes[route.index()].deliveries.to_vec();
    deliveries.push(delivery);
    prepared.graph.routes[route.index()].deliveries = deliveries.into_boxed_slice();
    prepared.graph.links.push(LinkSpec {
        from: source,
        to: wire,
        kind: delivery.link_kind(),
        callback: CallbackEdgeRef { route, sequence },
    });
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
    let plan = &prepared.graph.plans.classic_nets[0];
    let receiver = plan
        .inputs
        .iter()
        .find(|input| prepared.graph.nodes[input.receiver.index()].position == Some(receiver_position))
        .unwrap()
        .receiver;
    assert!(
        plan.inputs
            .iter()
            .any(|input| input.receiver == receiver && input.source_attenuation < 15)
    );
    let mut classic = backend(prepared.graph);
    let ComponentState::Electrical(state) = &mut classic.state.components[source.index()] else {
        unreachable!()
    };
    state.powered = true;
    state.output_strength = 15;
    assert_eq!(classic.wire_state(wire).power, 0);
    assert_eq!(
        classic.input_strength(receiver, InputPort::Main),
        15,
        "direct ingress is live before dust commit"
    );
    let shape = classic.program.classic().entries.iter().position(|entry| !entry.commit).unwrap();
    classic.run_classic_entry(shape);
    assert_eq!(classic.wire_state(wire).power, 0, "shape cannot commit signal");
    let update = classic.program.classic().entries.iter().position(|entry| entry.commit).unwrap();
    classic.run_classic_entry(update);
    assert_eq!(classic.wire_state(wire).power, 15);
}

#[test]
fn classic_captured_guard_without_capture_is_inert_and_plan_publication_is_cancelled() {
    let mut prepared = prepare(&line(4));
    let wires = prepared.graph.wires.iter().map(|wire| wire.node).collect::<FxHashSet<_>>();
    for route in &mut prepared.graph.routes {
        if wires.contains(&route.source) {
            continue;
        }
        for delivery in &mut route.deliveries {
            if matches!(prepared.graph.sites[delivery.target.index()].source,SiteSource::Component(node) if wires.contains(&node)) {
                let guard = GuardId::new(prepared.graph.guards.len() as u32);
                prepared.graph.guards.push(Guard::Site {
                    site: delivery.target,
                    view: OperandView::Captured,
                    test: SiteTest::IsWire,
                });
                delivery.guard = guard;
            }
        }
    }
    for link in &mut prepared.graph.links {
        if !wires.contains(&link.to) || wires.contains(&link.from) {
            continue;
        }
        let delivery = prepared.graph.routes[link.callback.route.index()]
            .deliveries
            .iter()
            .find(|delivery| delivery.sequence == link.callback.sequence && delivery.receiver == Some(CallbackReceiver::Component(link.to)))
            .unwrap();
        link.kind = delivery.link_kind();
    }
    prepared.graph.validate().unwrap();
    optimize(&mut prepared.graph, &prepared.entry);
    assert!(prepared.graph.plans.classic_nets.is_empty(), "missing capture cannot activate entry");
    let mut prepared = prepare(&line(4));
    let saved = prepared.graph.clone();
    let monitor = TaskMonitor::default();
    monitor.cancel();
    assert!(
        passes::compile_classic_redstone::CompileClassicRedstone
            .run(&mut prepared.graph, &PassContext::new(&monitor, &Default::default(), &prepared.entry))
            .is_err()
    );
    assert_eq!(prepared.graph, saved);
    let world = line(33);
    let mut prepared = passes::rich::prepare(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(63, 31, 31)),
        vec![],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap();
    optimize(&mut prepared.graph, &prepared.entry);
    assert!(prepared.graph.plans.classic_nets.is_empty(), "whole oversized region remains general");
}

#[test]
fn classic_declines_reentrant_ingress_prefix_before_signal_commit() {
    let mut world = line(4);
    let remote = BlockPos::new(20, 20, 20);
    world.set_block(remote.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(remote, Block::RedstoneWire { wire: Default::default() });
    let mut prepared = prepare(&world);
    let members = prepared
        .graph
        .wires
        .iter()
        .filter(|wire| prepared.graph.nodes[wire.node.index()].position != Some(remote))
        .map(|wire| wire.node)
        .collect::<Vec<_>>();
    let plan = ClassicAnalysis::new(&prepared.graph).infer(&members).unwrap();
    let route = prepared.graph.nodes[plan.source.index()].output_route.unwrap();
    let foreign = prepared
        .graph
        .wires
        .iter()
        .find(|wire| prepared.graph.nodes[wire.node.index()].position == Some(remote))
        .unwrap()
        .node;
    let mut prefix =
        prepared.graph.routes[route.index()].deliveries[plan.entries.iter().find(|entry| entry.commit).unwrap().ordinal as usize].clone();
    prefix.receiver = Some(CallbackReceiver::Component(foreign));
    let mut deliveries = prepared.graph.routes[route.index()].deliveries.to_vec();
    deliveries.insert(0, prefix);
    prepared.graph.routes[route.index()].deliveries = deliveries.into_boxed_slice();
    assert_eq!(
        ClassicAnalysis::new(&prepared.graph).infer(&members).unwrap_err(),
        "ingress route prefix may synchronously emit or reenter"
    );
}

#[test]
fn classic_imported_receiver_ticks_cover_powered_pending_states_and_continuation_cuts() {
    let bounds = (BlockPos::new(0, 0, 0), BlockPos::new(31, 31, 31));
    for torch in [false, true] {
        for powered in [false, true] {
            for pending in [false, true] {
                let mut native = line(2);
                let receiver_position = if torch { BlockPos::new(8, 21, 5) } else { BlockPos::new(8, 20, 5) };
                if torch {
                    native.set_block(BlockPos::new(8, 20, 5), Block::Stone {});
                    native.set_block(receiver_position, Block::RedstoneTorch { lit: powered });
                } else {
                    let Block::RedstoneRepeater { mut repeater } = native.get_block(receiver_position) else {
                        unreachable!()
                    };
                    repeater.powered = powered;
                    native.set_block(receiver_position, Block::RedstoneRepeater { repeater });
                }
                if pending {
                    native.schedule_tick(receiver_position, 2, TickPriority::Highest);
                }
                let ticks = native.scheduler().iter_entries().collect::<Vec<_>>();
                let general_options = crate::redpiler::CompilerOptions {
                    optimization_level: Some(0),
                    ..Default::default()
                };
                let classic_options = crate::redpiler::CompilerOptions {
                    optimization_level: Some(2),
                    classic_redstone: true,
                    ..Default::default()
                };
                let mut general =
                    GraphBackend::compile_with_options(&native, bounds, ticks.clone(), &general_options, Arc::new(TaskMonitor::default())).unwrap();
                let mut classic = GraphBackend::compile_with_pipeline(
                    &native,
                    bounds,
                    ticks,
                    &classic_options,
                    &local_net_pipeline(&classic_options),
                    Arc::new(TaskMonitor::default()),
                )
                .unwrap();
                assert!(
                    !classic.program.classic().nets.is_empty(),
                    "torch={torch} powered={powered} pending={pending}"
                );
                for step in 0..8 {
                    if step % 2 == 0 {
                        general.use_block(LEVER_POS).unwrap();
                        classic.use_block(LEVER_POS).unwrap();
                        set_native_lever(&mut native, LEVER_POS, (step / 2) % 2 == 0);
                    }
                    assert_same(&general, &classic);
                    assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
                    let mut projected = fixture_world();
                    classic.project_future(&mut projected);
                    for (&position, &node) in &classic.positions {
                        assert_eq!(projected.get_block(position), native.get_block(position));
                        assert_eq!(classic.node_block(node), native.get_block(position));
                    }
                    assert_eq!(
                        projected.scheduler().iter_entries().collect::<Vec<_>>(),
                        native.scheduler().iter_entries().collect::<Vec<_>>()
                    );
                    general.tick().unwrap();
                    classic.tick().unwrap();
                    native.tick_interpreted();
                }
            }
        }
    }
}

#[test]
fn classic_indexed_readers_reject_indirect_side_input_even_without_callback_edge() {
    let world = line(4);
    let prepared = prepare(&world);
    let members = prepared.graph.wires.iter().map(|wire| wire.node).collect::<Vec<_>>();
    let baseline = ClassicAnalysis::new(&prepared.graph).infer(&members).unwrap();
    let receiver = baseline.inputs[0].receiver;
    let wire_site = prepared
        .graph
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(members[0]))
        .unwrap();
    let conductor_site = prepared
        .graph
        .sites
        .iter()
        .position(|site| match site.source {
            SiteSource::Constant(profile) => prepared.graph.profiles[profile.index()].solid,
            _ => false,
        })
        .unwrap();
    let always = prepared.graph.guards.iter().position(|guard| matches!(guard, Guard::Always)).unwrap();
    let mut near = prepared.graph.clone();
    near.power_links.push(PowerLink {
        from: SiteId::new(wire_site as u32),
        from_port: PowerPort::Stored,
        to: PowerTarget::Conductor {
            site: SiteId::new(conductor_site as u32),
            include_dust: true,
        },
        face: BlockFace::Top,
        attenuation: 0,
        guard: GuardId::new(always as u32),
    });
    near.power_links.push(PowerLink {
        from: SiteId::new(conductor_site as u32),
        from_port: PowerPort::Conducted { include_dust: true },
        to: PowerTarget::Input {
            node: receiver,
            port: InputPort::Side,
        },
        face: BlockFace::Top,
        attenuation: 0,
        guard: GuardId::new(always as u32),
    });
    assert_eq!(
        ClassicAnalysis::new(&near).infer(&members).unwrap_err(),
        "private wire read through side or quasi port"
    );
}

#[test]
fn classic_merges_callback_neighbor_branches_with_shared_ingress() {
    let mut native = line(4);
    for z in 6..10 {
        let position = BlockPos::new(5, 20, z);
        native.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        native.set_block(position, Block::RedstoneWire { wire: Default::default() });
    }
    let receiver = BlockPos::new(5, 20, 10);
    native.set_block(receiver.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(
        receiver,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::North,
                delay: 2,
                powered: false,
                locked: false,
            },
        },
    );
    for z in 6..10 {
        let position = BlockPos::new(5, 20, z);
        let wire = redstone::wire::get_state_for_placement(&native, position);
        native.set_block(position, Block::RedstoneWire { wire });
    }
    let mut prepared = prepare(&native);
    let first_branch = prepared
        .graph
        .wires
        .iter()
        .filter(|wire| prepared.graph.nodes[wire.node.index()].position.unwrap().z == 5)
        .map(|wire| wire.node)
        .collect::<Vec<_>>();
    assert!(ClassicAnalysis::new(&prepared.graph).infer(&first_branch).is_err());
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
    assert_eq!(prepared.graph.plans.classic_nets[0].wires.len(), 8);
    prepared.graph.validate().unwrap();
    let mut classic = backend(prepared.graph);
    for enabled in [true, false, true, false] {
        general.use_block(LEVER_POS).unwrap();
        classic.use_block(LEVER_POS).unwrap();
        set_native_lever(&mut native, LEVER_POS, enabled);
        for _ in 0..4 {
            assert_same(&general, &classic);
            assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
            for (&position, &node) in &classic.positions {
                assert_eq!(classic.node_block(node), native.get_block(position));
            }
            general.tick().unwrap();
            classic.tick().unwrap();
            native.tick_interpreted();
        }
    }
    assert!(classic.counts.classic_net_invocations > 0);
    assert_eq!(classic.counts.wire_walks, 0);
}

fn independent_callback_lines() -> PlotWorld {
    let mut world = line(4);
    let source = BlockPos::new(5, 20, 7);
    world.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(source, lever(false));
    for x in 6..10 {
        let pos = BlockPos::new(x, 20, 7);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(pos, Block::RedstoneWire { wire: Default::default() });
    }
    let receiver = BlockPos::new(10, 20, 7);
    world.set_block(receiver.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        receiver,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::West,
                delay: 2,
                powered: false,
                locked: false,
            },
        },
    );
    for x in 6..10 {
        let pos = BlockPos::new(x, 20, 7);
        world.set_block(
            pos,
            Block::RedstoneWire {
                wire: redstone::wire::get_state_for_placement(&world, pos),
            },
        );
    }
    world
}

#[test]
fn classic_guarded_callback_lines_preserve_live_future_and_deopt_before_fresh_input() {
    let world = independent_callback_lines();
    let mut prepared = prepare(&world);
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 2);
    assert!(prepared.graph.plans.classic_nets.iter().all(|net| !net.guarded_wires.is_empty()));
    let mut forged = prepared.graph.clone();
    forged.plans.classic_nets[0].guarded_wires = Box::new([]);
    assert!(forged.validate().is_err());
    let mut classic = backend(prepared.graph);
    for graph in [&mut general, &mut classic] {
        graph.use_block(LEVER_POS).unwrap();
    }
    for _ in 0..3 {
        assert_same(&general, &classic);
        general.tick().unwrap();
        classic.tick().unwrap();
    }
    assert!(classic.state.classic_active.iter().all(|active| *active));
    let source_b = BlockPos::new(5, 20, 7);
    // The independent boundary's strength may differ from its saved proof state.
    // Commit B normally, then exercise both A polarities while B stays at 15.
    for position in [source_b, LEVER_POS, LEVER_POS] {
        for graph in [&mut general, &mut classic] {
            graph.use_block(position).unwrap();
        }
        for _ in 0..3 {
            assert_same(&general, &classic);
            general.tick().unwrap();
            classic.tick().unwrap();
        }
    }
    assert!(classic.state.classic_active.iter().all(|active| *active));
    // A fresh source output exists without its delivery. A's entry must reject the
    // compact episode BEFORE committing A's new zero and restore A's old 15.
    for graph in [&mut general, &mut classic] {
        let node = graph.positions[&source_b];
        let ComponentState::Electrical(state) = &mut graph.state.components[node.index()] else {
            unreachable!()
        };
        state.powered = false;
        state.output_strength = 0;
        graph.use_block(LEVER_POS).unwrap();
    }
    assert_same(&general, &classic);
    assert!(classic.state.classic_active.iter().any(|active| !*active));
    for _ in 0..8 {
        general.tick().unwrap();
        classic.tick().unwrap();
        assert_same(&general, &classic);
    }
    // Deoptimized state survives clone/inspection and projects the scalar future.
    assert_eq!(classic.state, classic.state.clone());
    let mut resumed = independent_callback_lines();
    let mut expected = independent_callback_lines();
    classic.reset(&mut resumed);
    general.reset(&mut expected);
    for _ in 0..8 {
        resumed.tick_interpreted();
        expected.tick_interpreted();
        for position in [
            LEVER_POS,
            source_b,
            BlockPos::new(6, 20, 5),
            BlockPos::new(10, 20, 5),
            BlockPos::new(10, 20, 7),
        ] {
            assert_eq!(resumed.get_block(position), expected.get_block(position));
        }
        assert_eq!(
            resumed.scheduler().iter_entries().collect::<Vec<_>>(),
            expected.scheduler().iter_entries().collect::<Vec<_>>()
        );
    }
}

#[test]
fn classic_general_wire_write_deopts_in_the_original_walk() {
    let world = line(4);
    let mut prepared = prepare(&world);
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    let mut classic = backend(prepared.graph);
    let root = BlockPos::new(6, 20, 5);
    for graph in [&mut general, &mut classic] {
        let source = graph.positions[&LEVER_POS];
        let ComponentState::Electrical(state) = &mut graph.state.components[source.index()] else {
            unreachable!()
        };
        state.powered = true;
        state.output_strength = 15;
        graph.evaluate_seed(graph.positions[&root], None).unwrap();
    }
    assert_same(&general, &classic);
    assert_eq!(classic.state.classic_active.as_ref(), &[false]);
    assert_eq!(classic.state.classic_signals.as_ref(), &[0], "deopt must not commit newest source");
    assert_eq!(classic.wire_state(classic.positions[&root]).power, 15);
}

#[test]
fn classic_analysis_reads_typed_wire_sides_and_constant_profile_flags() {
    let mut graph = prepare(&independent_callback_lines()).graph;
    let node_at = |graph: &CompileGraph, pos| {
        graph
            .nodes
            .iter()
            .position(|node| node.position == Some(pos))
            .map(|index| NodeId::new(index as u32))
            .unwrap()
    };
    let root = node_at(&graph, BlockPos::new(6, 20, 5));
    let receiver = node_at(&graph, BlockPos::new(10, 20, 7));
    let other_source = node_at(&graph, BlockPos::new(5, 20, 7));
    let site_for =
        |graph: &CompileGraph, node| SiteId::new(graph.sites.iter().position(|site| site.source == SiteSource::Component(node)).unwrap() as u32);
    let root_site = site_for(&graph, root);
    let source_site = site_for(&graph, other_source);
    let ComponentState::Wire(wire) = &mut graph.nodes[root.index()].state else {
        unreachable!()
    };
    wire.north = RedstoneWireSide::Side;
    wire.south = RedstoneWireSide::Side;
    // This deliberately differs from saved raw_state. The actual state owner
    // controls directional emission, so dependency inference must retain it.
    graph.power_links.push(PowerLink {
        from: root_site,
        from_port: PowerPort::Weak(BlockFace::North),
        to: PowerTarget::Input {
            node: receiver,
            port: InputPort::Main,
        },
        face: BlockFace::North,
        attenuation: 0,
        guard: GuardId::new(0),
    });
    assert!(ClassicAnalysis::new(&graph).direct_wire_sources(receiver).unwrap().contains(&root));
    let mut profile = graph.profiles[0];
    profile.raw_state = Block::Air.get_id();
    profile.solid = true;
    profile.transparent = false;
    let profile_id = ProfileId::new(graph.profiles.len() as u32);
    graph.profiles.push(profile);
    let site = SiteId::new(graph.sites.len() as u32);
    graph.sites.push(SiteSpec {
        position: BlockPos::new(30, 30, 30),
        source: SiteSource::Constant(profile_id),
        source_strength: 0,
    });
    let guard = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Site {
        site,
        view: OperandView::Live,
        test: SiteTest::Solid,
    });
    graph.power_links.push(PowerLink {
        from: source_site,
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: root,
            port: InputPort::Main,
        },
        face: BlockFace::North,
        attenuation: 0,
        guard,
    });
    assert!(ClassicAnalysis::new(&graph).direct_wire_sources(root).unwrap().contains(&other_source));
    graph.guards[guard.index()] = Guard::Site {
        site,
        view: OperandView::Live,
        test: SiteTest::Transparent,
    };
    assert!(!ClassicAnalysis::new(&graph).direct_wire_sources(root).unwrap().contains(&other_source));
}

fn two_input_torch() -> PlotWorld {
    let mut world = line(4);
    world.set_block(BlockPos::new(10, 20, 5), Block::Stone {});
    world.set_block(BlockPos::new(10, 21, 5), Block::RedstoneTorch { lit: true });
    for position in [BlockPos::new(10, 20, 6), BlockPos::new(10, 20, 7)] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    world.set_block(BlockPos::new(10, 20, 7), lever(false));
    world.set_block(BlockPos::new(10, 20, 6), Block::RedstoneWire { wire: Default::default() });
    for position in (6..10).map(|x| BlockPos::new(x, 20, 5)).chain([BlockPos::new(10, 20, 6)]) {
        let wire = redstone::wire::get_state_for_placement(&world, position);
        world.set_block(position, Block::RedstoneWire { wire });
    }
    world
}

#[test]
fn classic_guarded_independent_main_stays_live_and_falls_back_before_commit() {
    let mut native = two_input_torch();
    let mut prepared = prepare(&native);
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    let source = prepared
        .graph
        .nodes
        .iter()
        .position(|node| node.position == Some(LEVER_POS))
        .map(|index| NodeId::new(index as u32))
        .unwrap();
    let plan = prepared
        .graph
        .plans
        .classic_nets
        .iter()
        .find(|plan| plan.source == source)
        .expect("A must have a certified local cut");
    assert!(!plan.outside.is_empty());
    assert!(plan.inputs.iter().any(|input| input.guarded));
    let receiver = plan.inputs.iter().find(|input| input.guarded).unwrap().receiver;
    let mut forged = prepared.graph.clone();
    let forged_plan = forged.plans.classic_nets.iter_mut().find(|plan| plan.source == source).unwrap();
    forged_plan.outside = Box::new([]);
    assert!(forged.validate().is_err(), "independent validation must reject missing outside guards");

    let mut classic = backend(prepared.graph);
    assert_eq!(
        classic.program.classic().input_by_node[receiver.index()],
        None,
        "independent Main must never get a direct override"
    );
    for (position, enabled) in [
        (LEVER_POS, true),
        (BlockPos::new(10, 20, 7), true),
        (LEVER_POS, false),
        (BlockPos::new(10, 20, 7), false),
    ] {
        for graph in [&mut general, &mut classic] {
            graph.use_block(position).unwrap();
        }
        set_native_lever(&mut native, position, enabled);
        for _ in 0..4 {
            assert_same(&general, &classic);
            assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
            for (&position, &node) in &classic.positions {
                assert_eq!(classic.node_block(node), native.get_block(position));
            }
            general.tick().unwrap();
            classic.tick().unwrap();
            native.tick_interpreted();
        }
    }
    assert!(classic.counts.classic_net_invocations > 0);
    assert!(classic.state.classic_active.iter().any(|active| !*active));
    // Cut with a real pending delayed receiver after deoptimization.
    classic.use_block(BlockPos::new(10, 20, 7)).unwrap();
    set_native_lever(&mut native, BlockPos::new(10, 20, 7), true);
    let positions = classic.positions.keys().copied().collect::<Vec<_>>();
    let mut resumed = two_input_torch();
    classic.reset(&mut resumed);
    assert_eq!(
        resumed.scheduler().iter_entries().collect::<Vec<_>>(),
        native.scheduler().iter_entries().collect::<Vec<_>>()
    );
    for _ in 0..8 {
        resumed.tick_interpreted();
        native.tick_interpreted();
        for &position in &positions {
            assert_eq!(resumed.get_block(position), native.get_block(position));
        }
        assert_eq!(
            resumed.scheduler().iter_entries().collect::<Vec<_>>(),
            native.scheduler().iter_entries().collect::<Vec<_>>()
        );
    }
}

#[test]
fn classic_declines_structural_outside_dot_even_if_direction_currently_omitted() {
    let mut world = two_input_torch();
    let position = BlockPos::new(25, 20, 25);
    world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(position, Block::RedstoneWire { wire: Default::default() });
    let raw = redstone::wire::get_raw_sides_from(Default::default(), position, |position| world.get_block(position));
    assert!(redstone::wire::is_dot(raw), "the negative witness must really permit dot/cross choice");
    let mut prepared = prepare(&world);
    let source = prepared
        .graph
        .nodes
        .iter()
        .position(|node| node.position == Some(LEVER_POS))
        .map(|index| NodeId::new(index as u32))
        .unwrap();
    let receiver = prepared
        .graph
        .nodes
        .iter()
        .position(|node| node.position == Some(BlockPos::new(10, 21, 5)))
        .map(|index| NodeId::new(index as u32))
        .unwrap();
    let from = SiteId::new(prepared.graph.sites.iter().position(|site| site.position == position).unwrap() as u32);
    // A structural directional alternative may be omitted by the saved dot.
    // Retain it in discovery even without a callback edge; a future cross could emit.
    prepared.graph.power_links.push(PowerLink {
        from,
        from_port: PowerPort::Weak(BlockFace::North),
        to: PowerTarget::Input {
            node: receiver,
            port: InputPort::Main,
        },
        face: BlockFace::North,
        attenuation: 0,
        guard: GuardId::new(0),
    });
    optimize(&mut prepared.graph, &prepared.entry);
    assert!(!prepared.graph.plans.classic_nets.iter().any(|plan| plan.source == source));
}

#[test]
fn classic_failed_reference_restores_writes_beyond_proposed_guards() {
    let mut world = line(4);
    for z in [6, 7] {
        let position = BlockPos::new(5, 20, z);
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(position, Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(BlockPos::new(5, 19, 8), Block::Stone {});
    world.set_block(BlockPos::new(5, 20, 8), lever(true));
    world.set_block(
        BlockPos::new(5, 20, 9),
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFace::East.into(),
                sticky: false,
                extended: false,
            },
        },
    );
    for z in [6, 7] {
        let position = BlockPos::new(5, 20, z);
        let mut wire = redstone::wire::get_state_for_placement(&world, position);
        wire.power = 0; // Preserve intentionally stale C until the rejected episode reaches it.
        world.set_block(position, Block::RedstoneWire { wire });
    }
    let mut graph = prepare(&world).graph;
    let source = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(LEVER_POS))
        .map(|index| NodeId::new(index as u32))
        .unwrap();
    let b = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(BlockPos::new(5, 20, 6)))
        .map(|index| NodeId::new(index as u32))
        .unwrap();
    // An intentionally inconsistent near-match removes the declared electrical
    // dependency while native seed geometry still sees the lever. The reference
    // must reject it and undo C's unexpected write outside the proposed guards.
    graph.power_links.retain(|link| {
        !(link.to
            == PowerTarget::Input {
                node: b,
                port: InputPort::Main,
            }
            && graph.sites[link.from.index()].source == SiteSource::Component(source))
    });
    let members = graph
        .wires
        .iter()
        .filter(|wire| graph.nodes[wire.node.index()].position.unwrap().z == 5)
        .map(|wire| wire.node)
        .collect::<Vec<_>>();
    let mut plan = ClassicAnalysis::new(&graph).infer(&members).unwrap();
    assert!(
        plan.guarded_wires
            .iter()
            .all(|node| graph.nodes[node.index()].position != Some(BlockPos::new(5, 20, 7)))
    );
    let mut probe = backend(graph.clone());
    for &node in &plan.guarded_wires {
        let wire_plan = probe.wire_plan(node).clone();
        let state = probe.wire_state(node);
        let raw = probe.raw_wire_sides(&wire_plan, state);
        assert_eq!(
            probe.seed_power(&wire_plan).0,
            state.power,
            "the poisoned fixture must enter replay with stable guard power"
        );
        assert_eq!(redstone::wire::regulate_sides(state, raw), state);
    }
    let mut reference = super::super::classic::ClassicReference::new(&graph).unwrap();
    let reason = reference.prove(&mut plan, &TaskMonitor::default()).unwrap_err();
    println!("deliberately rejected poisoned reference: {reason}");
    assert!(
        reference.has_initial_state(),
        "failed proof must restore all actual writes, including unowned C"
    );
    assert!(
        reference.restored_unowned_writes() > 0,
        "the negative episode must actually write beyond its declared guard set"
    );
}

#[test]
fn classic_analog_comparator_source_and_receiver_preserve_samples_pending_and_continuation() {
    let mut world = line(1);
    for position in [LEVER_POS, BlockPos::new(7, 20, 5)] {
        world.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
            },
        );
    }
    let mut prepared = prepare(&world);
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
    let source = prepared.graph.plans.classic_nets[0].source;
    assert!(
        prepared.graph.plans.classic_nets[0]
            .entries
            .iter()
            .flat_map(|entry| entry.ops.iter())
            .any(|op| matches!(op.sample, ClassicSample::Comparator { .. }))
    );
    let mut forged = prepared.graph.clone();
    let op = forged.plans.classic_nets[0]
        .entries
        .iter_mut()
        .flat_map(|entry| entry.ops.iter_mut())
        .next()
        .unwrap();
    op.sample = ClassicSample::Comparator { strength: 16, powered: true };
    assert!(forged.validate().is_err());
    let mut classic = backend(prepared.graph);
    for strength in [1, 14, 3, 15, 0, 7, 7, 0] {
        for engine in [&mut general, &mut classic] {
            engine.set_node(source, strength > 0, strength, None, false).unwrap();
            engine.fire_route(source).unwrap();
        }
        assert_same(&general, &classic);
        // No ticks between pulses: the receiver's pending membership stays live.
    }
    assert!(classic.counts.classic_net_invocations > 0);
    assert_eq!(classic.counts.wire_walks, 0);
    let mut normal_world = line(1);
    for position in [LEVER_POS, BlockPos::new(7, 20, 5)] {
        normal_world.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
            },
        );
    }
    let mut classic_world = world;
    general.reset(&mut normal_world);
    classic.reset(&mut classic_world);
    for _ in 0..8 {
        normal_world.tick_interpreted();
        classic_world.tick_interpreted();
        assert_eq!(
            normal_world.scheduler().iter_entries().collect::<Vec<_>>(),
            classic_world.scheduler().iter_entries().collect::<Vec<_>>()
        );
        for position in [LEVER_POS, BlockPos::new(6, 20, 5), BlockPos::new(7, 20, 5)] {
            assert_eq!(normal_world.get_block(position), classic_world.get_block(position));
            assert_eq!(
                serde_json::to_value(normal_world.get_block_entity(position)).unwrap(),
                serde_json::to_value(classic_world.get_block_entity(position)).unwrap()
            );
        }
    }
}

#[test]
fn classic_comparator_receiver_matches_native_live_and_captured_callbacks() {
    let mut native = line(1);
    let receiver = BlockPos::new(7, 20, 5);
    native.set_block(
        receiver,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Subtract, false),
        },
    );
    let mut prepared = prepare(&native);
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
    let mut classic = backend(prepared.graph);
    for step in 0..6 {
        general.use_block(LEVER_POS).unwrap();
        classic.use_block(LEVER_POS).unwrap();
        set_native_lever(&mut native, LEVER_POS, step % 2 == 0);
        for _ in 0..3 {
            assert_same(&general, &classic);
            assert_eq!(classic.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
            for (&position, &node) in &classic.positions {
                assert_eq!(classic.node_block(node), native.get_block(position));
            }
            general.tick().unwrap();
            classic.tick().unwrap();
            native.tick_interpreted();
        }
    }
    assert_eq!(classic.counts.wire_walks, 0);
    assert!(classic.counts.classic_receiver_ops > 0);
}

#[test]
fn classic_first_two_comparator_samples_preserve_first_effective_fifo_for_all_stored_pairs() {
    let comparator = NodeId::new(0);
    let torch = NodeId::new(1);
    for a in [0, 3, 15] {
        for b in [0, 3, 15] {
            for c in [0, 3, 15] {
                let sample = |strength| ClassicSample::Comparator {
                    strength,
                    powered: strength > 0,
                };
                let original = vec![
                    (comparator, sample(a)),
                    (torch, ClassicSample::Boolean { nonzero: true }),
                    (comparator, sample(b)),
                    (comparator, sample(c)),
                    (torch, ClassicSample::Boolean { nonzero: false }),
                ];
                let mut compact = original.clone();
                super::classic::compact_classic_samples(&mut compact);
                for stored in 0..16 {
                    for powered in [false, true] {
                        for pending in 0u8..4 {
                            for torch_powered in [false, true] {
                                let schedule = |samples: &[(NodeId, ClassicSample)]| {
                                    let mut pending = pending;
                                    let mut result = vec![];
                                    for &(node, sample) in samples {
                                        let bit = 1 << node.index();
                                        if pending & bit != 0 {
                                            continue;
                                        }
                                        let mismatch = match sample {
                                            ClassicSample::Comparator { strength, powered: desired } => strength != stored || desired != powered,
                                            ClassicSample::Boolean { nonzero } => nonzero == torch_powered,
                                        };
                                        if mismatch {
                                            pending |= bit;
                                            result.push(node);
                                        }
                                    }
                                    result
                                };
                                assert_eq!(schedule(&original), schedule(&compact));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn classic_comparator_source_accepts_all_analog_strengths_without_binary_receiver_payloads() {
    let mut world = line(4);
    world.set_block(
        LEVER_POS,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
        },
    );
    let mut prepared = prepare(&world);
    let mut general = backend(prepared.graph.clone());
    optimize(&mut prepared.graph, &prepared.entry);
    assert_eq!(prepared.graph.plans.classic_nets.len(), 1);
    let source = prepared.graph.plans.classic_nets[0].source;
    let mut classic = backend(prepared.graph);
    for strength in 0..16 {
        for engine in [&mut general, &mut classic] {
            engine.set_node(source, strength > 0, strength, None, false).unwrap();
            engine.fire_route(source).unwrap();
        }
        assert_same(&general, &classic);
    }
    assert_eq!(classic.counts.wire_walks, 0);
}

#[test]
fn classic_comparator_receiver_declines_overrides_and_variable_side() {
    for override_input in [false, true] {
        let mut world = line(8);
        let position = BlockPos::new(14, 20, 5);
        world.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
            },
        );
        if !override_input {
            world.set_block(BlockPos::new(14, 19, 6), Block::Stone {});
            world.set_block(
                BlockPos::new(14, 20, 6),
                Block::RedstoneRepeater {
                    repeater: RedstoneRepeater {
                        facing: BlockDirection::South,
                        ..Default::default()
                    },
                },
            );
        }
        let mut prepared = prepare(&world);
        if override_input {
            let receiver = prepared.graph.nodes.iter().position(|node| node.position == Some(position)).unwrap();
            prepared
                .graph
                .comparator_inputs
                .iter_mut()
                .find(|input| input.node.index() == receiver)
                .unwrap()
                .direct_override = Some(ComparatorOverride::Literal(0));
        }
        optimize(&mut prepared.graph, &prepared.entry);
        assert!(prepared.graph.plans.classic_nets.is_empty(), "override={override_input}");
    }
}

#[test]
fn classic_zero_side_declines_structural_dot_even_when_saved_emission_is_zero() {
    for comparator in [false, true] {
        let mut world = line(1);
        let receiver_position = BlockPos::new(7, 20, 5);
        if comparator {
            world.set_block(
                receiver_position,
                Block::RedstoneComparator {
                    comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
                },
            );
        }
        let remote = BlockPos::new(25, 20, 25);
        world.set_block(remote.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(remote, Block::RedstoneWire { wire: Default::default() });
        assert!(redstone::wire::is_dot(redstone::wire::get_raw_sides_from(
            Default::default(),
            remote,
            |position| world.get_block(position)
        )));
        let mut prepared = prepare(&world);
        let receiver = NodeId::new(
            prepared
                .graph
                .nodes
                .iter()
                .position(|node| node.position == Some(receiver_position))
                .unwrap() as u32,
        );
        let from = SiteId::new(prepared.graph.sites.iter().position(|site| site.position == remote).unwrap() as u32);
        prepared.graph.power_links.push(PowerLink {
            from,
            from_port: PowerPort::Weak(BlockFace::North),
            to: PowerTarget::Input {
                node: receiver,
                port: InputPort::Side,
            },
            face: BlockFace::North,
            attenuation: 0,
            guard: GuardId::new(0),
        });
        let analysis = ClassicAnalysis::new(&prepared.graph);
        assert_eq!(
            analysis.census_expression(receiver, InputPort::Side).unwrap(),
            (0, vec![]),
            "saved dot must hide directional contribution"
        );
        optimize(&mut prepared.graph, &prepared.entry);
        assert!(
            !prepared
                .graph
                .plans
                .classic_nets
                .iter()
                .any(|net| net.inputs.iter().any(|input| input.receiver == receiver))
        );
    }
}
