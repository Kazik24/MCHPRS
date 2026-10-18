//! Boundary state, future work and projected dust, independently checked against native.
use super::*;
use crate::redpiler::passes::{PassContext, planning::PlanBuilder};

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
        .run(
            graph,
            &PassContext {
                monitor: &monitor,
                options: &Default::default(),
                entry,
            },
        )
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

#[cfg(feature = "native-demo")]
#[test]
#[ignore = "requires native-demo Clang/lld environment; checks classic plus O3 through DLL"]
fn classic_native_dll_matches_rust_and_interpreter() {
    let clang = std::path::PathBuf::from(std::env::var_os("MCHPRS_NATIVE_CLANG").expect("MCHPRS_NATIVE_CLANG"));
    let linker = std::env::var_os("MCHPRS_NATIVE_LINKER").map(std::path::PathBuf::from);
    let artifacts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/native-query-demo/classic-test");
    let mut native = line(4);
    let mut projected = line(4);
    let options = crate::redpiler::CompilerOptions {
        optimization_level: Some(3),
        classic_redstone: true,
        ..Default::default()
    };
    let mut compiled = GraphBackend::compile_with_options(
        &native,
        (BlockPos::new(0, 0, 0), BlockPos::new(31, 31, 31)),
        vec![],
        &options,
        Arc::new(TaskMonitor::default()),
    )
    .unwrap();
    assert!(!compiled.program.classic().nets.is_empty());
    let first_stats = compiled.enable_native_graph_demo(&clang, linker.as_deref(), &artifacts).unwrap();
    assert!(!first_stats.cache_hit);
    let mut second = GraphBackend::compile_with_options(
        &native,
        (BlockPos::new(0, 0, 0), BlockPos::new(31, 31, 31)),
        vec![],
        &options,
        Arc::new(TaskMonitor::default()),
    )
    .unwrap();
    let second_stats = second.enable_native_graph_demo(&clang, linker.as_deref(), &artifacts).unwrap();
    assert!(second_stats.cache_hit);
    assert_eq!(first_stats.library, second_stats.library);
    let second_initial = second.state.clone();
    compiled.enable_native_graph_verification().unwrap();
    for step in 0..32 {
        if step % 3 == 0 {
            compiled.use_block(LEVER_POS).unwrap();
            set_native_lever(&mut native, LEVER_POS, (step / 3) % 2 == 0);
        }
        compiled.tick().unwrap();
        native.tick_interpreted();
        compiled.flush(&mut projected, false);
        for &position in compiled.positions.keys() {
            assert_eq!(projected.get_block(position), native.get_block(position));
        }
        assert_eq!(compiled.scheduled_ticks(), native.scheduler().iter_entries().collect::<Vec<_>>());
    }
    assert!(compiled.counts.classic_net_invocations > 0);
    assert!(compiled.counts.classic_receiver_ops > 0);
    assert_eq!(compiled.counts.wire_walks, 0);
    second.sync_native_graph(true);
    assert_eq!(second.state, second_initial, "shared code must not share mutable engine state");
    assert_eq!(second.future.clock.logical_tick, 0);
    assert!(second.scheduled_ticks().is_empty());
    compiled.reset(&mut projected);
    for _ in 0..8 {
        projected.tick_interpreted();
        native.tick_interpreted();
        for &position in compiled.positions.keys() {
            assert_eq!(projected.get_block(position), native.get_block(position));
        }
    }
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
        passes::run_graph_pipeline(graph, &options, &general_draft.entry, &passes::default_graph_pipeline(&options), &monitor).unwrap();
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
        assert!(prepared.graph.plans.classic_nets.is_empty(), "variant {variant}");
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
            .run(
                &mut prepared.graph,
                &PassContext {
                    monitor: &monitor,
                    options: &Default::default(),
                    entry: &prepared.entry
                }
            )
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
                let mut classic =
                    GraphBackend::compile_with_options(&native, bounds, ticks, &classic_options, Arc::new(TaskMonitor::default())).unwrap();
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
