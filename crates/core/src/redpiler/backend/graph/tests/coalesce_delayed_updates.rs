//! Exact state/future checks for the optional scheduling-only update rewrite.
use super::*;
use crate::redpiler::components::NodeState;
use crate::redpiler::passes::coalesce_delayed_updates::CoalesceDelayedUpdates;
use crate::redpiler::passes::{Pass, PassEntry, PassPipeline, run_graph_pipeline};
use crate::redpiler::program_graph::{CircuitNodeDraft, ComponentState, Guard, LinkSpec, PowerLink, SiteSpec};
use mchprs_blocks::blocks::{ComparatorMode, RedstoneComparator, RedstoneObserver};
use mchprs_blocks::{BlockDirection, BlockFacing};

fn row(node: u32, sequence: u32) -> RouteDelivery {
    RouteDelivery {
        target: SiteId::new(node),
        receiver: Some(CallbackReceiver::Component(NodeId::new(node))),
        kind: DeliveryKind::Update {
            direction: Some(BlockFace::North),
        },
        guard: GuardId::new(0),
        view: OperandView::Live,
        sequence,
    }
}

fn fixture(rows: Vec<RouteDelivery>) -> CompileGraph {
    let blocks = [
        Block::RedstoneBlock,
        Block::RedstoneTorch { lit: true },
        Block::RedstoneComparator {
            comparator: RedstoneComparator {
                facing: BlockDirection::North,
                mode: ComparatorMode::Compare,
                powered: false,
            },
        },
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::North,
                powered: false,
            },
        },
        Block::from_name("copper_bulb").unwrap().with_copper_bulb_state(false, false).unwrap(),
    ];
    let kinds = [
        CircuitNodeKind::Electrical(NodeType::Constant),
        CircuitNodeKind::Electrical(NodeType::Torch),
        CircuitNodeKind::Electrical(NodeType::Comparator {
            mode: ComparatorMode::Compare,
            far_input: None,
            facing_diode: false,
        }),
        CircuitNodeKind::Observer {
            watched: SiteId::new(0),
            facing: BlockFace::North,
        },
        CircuitNodeKind::Electrical(NodeType::CopperBulb),
    ];
    let mut graph = CompileGraph {
        nodes: blocks
            .into_iter()
            .zip(kinds)
            .enumerate()
            .map(|(index, (block, kind))| CircuitNodeDraft {
                position: Some(BlockPos::new(index as i32, 1, 1)),
                raw_state: Some(block.get_id()),
                kind,
                state: ComponentState::Electrical(NodeState::simple(index <= 1)),
                output_route: Some(RouteId::new(index as u32)),
            })
            .collect(),
        links: vec![],
        power_links: (1..=4)
            .map(|node| PowerLink {
                from: SiteId::new(0),
                from_port: PowerPort::Stored,
                to: PowerTarget::Input {
                    node: NodeId::new(node),
                    port: InputPort::Main,
                },
                face: BlockFace::North,
                attenuation: 0,
                guard: GuardId::new(0),
            })
            .collect(),
        comparator_inputs: vec![ComparatorInputPlan {
            node: NodeId::new(2),
            rear: SiteId::new(0),
            direct_override: None,
            far_override: None,
            far_threshold: 15,
        }],
        profiles: vec![],
        guards: vec![Guard::Always],
        contacts: vec![],
        routes: (0..5)
            .map(|node| Route {
                source: NodeId::new(node),
                deliveries: Box::new([]),
            })
            .collect(),
        wires: vec![],
        wire_relations: vec![],
        sites: (0..5)
            .map(|node| SiteSpec {
                position: BlockPos::new(node as i32, 1, 1),
                source: SiteSource::Component(NodeId::new(node)),
                source_strength: 0,
            })
            .collect(),
        pistons: vec![],
        resources: vec![],
        initial_pistons: vec![],
        initial_resources: vec![],
        plans: Default::default(),
    };
    graph.routes[0].deliveries = rows.into_boxed_slice();
    rebuild_links(&mut graph);
    graph
}

fn rebuild_links(graph: &mut CompileGraph) {
    graph.links = graph
        .routes
        .iter()
        .enumerate()
        .flat_map(|(index, route)| {
            route.deliveries.iter().filter_map(move |delivery| {
                let Some(CallbackReceiver::Component(node)) = delivery.receiver else {
                    return None;
                };
                Some(LinkSpec {
                    from: route.source,
                    to: node,
                    kind: delivery.link_kind(),
                    callback: CallbackEdgeRef {
                        route: RouteId::new(index as u32),
                        sequence: delivery.sequence,
                    },
                })
            })
        })
        .collect();
}

fn rewrite(graph: &mut CompileGraph) -> TaskMonitor {
    let monitor = TaskMonitor::default();
    run_graph_pipeline(
        graph,
        &Default::default(),
        &EntryState {
            logical_tick: 17,
            pending_ticks: vec![],
        },
        &PassPipeline::new(vec![PassEntry {
            pass: Pass::rewrite(CoalesceDelayedUpdates),
            enabled: true,
        }]),
        &monitor,
    )
    .unwrap();
    monitor
}

fn backend(graph: CompileGraph) -> GraphBackend {
    let (program, state) = graph.lower().unwrap();
    GraphBackend::new_unactivated(program, state, 17).unwrap()
}

fn same(left: &GraphBackend, right: &GraphBackend) {
    assert_eq!(left.state.components, right.state.components);
    assert_eq!(left.scheduled_ticks(), right.scheduled_ticks(), "deadline, priority and FIFO");
    assert_eq!(left.future.pending, right.future.pending);
}

#[test]
fn delayed_update_coalescing_preserves_pending_states_and_first_insertion_order() {
    let rows = vec![row(1, 4), row(2, 7), row(1, 9), row(3, 12), row(2, 15), row(3, 19), row(1, 21)];
    let graph = fixture(rows);
    let mut compact = graph.clone();
    let monitor = rewrite(&mut compact);
    assert_eq!(
        compact.routes[0].deliveries.iter().map(|row| row.sequence).collect::<Vec<_>>(),
        [4, 7, 12]
    );
    assert_eq!(
        monitor.graph_statistics().transformations[0].before.links - monitor.graph_statistics().transformations[0].after.links,
        4
    );
    let once = compact.clone();
    rewrite(&mut compact);
    assert_eq!(compact, once);
    for signal in [0, 1, 7, 15] {
        for powered in [false, true] {
            for pending in 0..8 {
                let mut general = backend(graph.clone());
                let mut optimized = backend(compact.clone());
                for engine in [&mut general, &mut optimized] {
                    if let ComponentState::Electrical(state) = &mut engine.state.components[0] {
                        state.output_strength = signal;
                    }
                    for node in 1..=3 {
                        if let ComponentState::Electrical(state) = &mut engine.state.components[node] {
                            state.powered = powered;
                            state.output_strength = u8::from(powered) * 15;
                        }
                        if pending & (1 << (node - 1)) != 0 {
                            engine.schedule(NodeId::new(node as u32), 3, TickPriority::Higher);
                        }
                    }
                    engine.deliver_route(RouteId::new(0), None).unwrap();
                }
                same(&general, &optimized);
                assert_eq!(general.counts.callbacks - optimized.counts.callbacks, 4);
                for _ in 0..8 {
                    general.tick().unwrap();
                    optimized.tick().unwrap();
                    same(&general, &optimized);
                }
            }
        }
    }
}

#[test]
fn immediate_output_changes_are_barriers_with_a_future_counterexample() {
    // A bulb toggles immediately. A later torch sample sees its new signal.
    let mut graph = fixture(vec![row(1, 0), row(4, 1), row(1, 2)]);
    graph.power_links[0].from = SiteId::new(4);
    let mut compact = graph.clone();
    rewrite(&mut compact);
    assert_eq!(compact, graph);
    let mut wrong = graph.clone();
    wrong.routes[0].deliveries = wrong.routes[0].deliveries[..2].to_vec().into_boxed_slice();
    rebuild_links(&mut wrong);
    let mut engines = [backend(graph), backend(compact), backend(wrong)];
    for engine in &mut engines {
        engine.deliver_route(RouteId::new(0), None).unwrap();
    }
    same(&engines[0], &engines[1]);
    assert!(engines[0].is_pending(NodeId::new(1)));
    assert!(
        !engines[2].is_pending(NodeId::new(1)),
        "global duplicate removal loses a meaningful sample"
    );
}

#[test]
fn captured_guarded_and_non_delayed_deliveries_split_runs() {
    let mut graph = fixture(vec![row(1, 0), row(1, 1), row(1, 2), row(1, 3), row(1, 4), row(4, 5), row(1, 6)]);
    graph.routes[0].deliveries[1].view = OperandView::Captured;
    graph.guards.push(Guard::All(vec![GuardId::new(0)].into_boxed_slice()));
    graph.routes[0].deliveries[3].guard = GuardId::new(1);
    rebuild_links(&mut graph);
    let before = graph.clone();
    rewrite(&mut graph);
    assert_eq!(graph, before);
    // Identical sequence IDs are still separate occurrences with multiset links.
    let mut duplicates = fixture(vec![row(1, 9), row(1, 9), row(1, 9)]);
    rewrite(&mut duplicates);
    assert_eq!(duplicates.links.len(), 1);
    assert_eq!(duplicates.routes[0].deliveries.len(), 1);
}

#[test]
fn implicit_receivers_preserve_link_multiplicity_and_turbo_routes_stay_unchanged() {
    for implicit_first in [false, true] {
        let mut rows = vec![row(1, 2), row(1, 3)];
        rows[usize::from(!implicit_first)].receiver = None;
        let mut graph = fixture(rows);
        rewrite(&mut graph);
        assert_eq!(graph.routes[0].deliveries.len(), 1);
        assert_eq!(graph.links.len(), usize::from(!implicit_first));
        graph.validate().unwrap();
    }

    let mut graph = fixture(vec![row(1, 2), row(1, 3)]);
    let node = NodeId::new(5);
    graph.nodes.push(CircuitNodeDraft {
        position: Some(BlockPos::new(5, 1, 1)),
        raw_state: Some(Block::RedstoneWire { wire: Default::default() }.get_id()),
        kind: CircuitNodeKind::Wire(0),
        state: ComponentState::Wire(Default::default()),
        output_route: Some(RouteId::new(0)),
    });
    graph.wires.push(WirePlan {
        node,
        canonical_neighbors: [SiteId::new(0); 24],
        shape_route: None,
        turbo_route: Some(RouteId::new(0)),
        execution: Default::default(),
    });
    graph.nodes[0].output_route = None;
    graph.routes[0].source = node;
    rebuild_links(&mut graph);
    let before = graph.clone();
    rewrite(&mut graph);
    assert_eq!(graph, before);
}

#[test]
fn cancellation_retains_routes_and_default_pipeline_selects_coalescing_at_o5() {
    let mut graph = fixture(vec![row(1, 0), row(1, 1), row(1, 2)]);
    let before = graph.clone();
    let monitor = TaskMonitor::default();
    monitor.cancel();
    let options = Default::default();
    let entry = EntryState {
        logical_tick: 17,
        pending_ticks: vec![],
    };
    assert!(passes::GraphPass::run(&CoalesceDelayedUpdates, &mut graph, &passes::PassContext::new(&monitor, &options, &entry)).is_err());
    assert_eq!(graph, before);
    for level in [Some(0), Some(1), Some(2), Some(3), Some(4), Some(5), None] {
        let options = crate::redpiler::CompilerOptions {
            optimization_level: level,
            ..Default::default()
        };
        let pipeline = passes::default_graph_pipeline(&options);
        let coalescing = pipeline
            .entries
            .iter()
            .position(|entry| matches!(&entry.pass, Pass::Rewrite(pass) if pass.name() == "experimental-coalesce-delayed-updates"))
            .unwrap();
        assert_eq!(pipeline.entries[coalescing].enabled, level.is_none_or(|level| level >= 5));
        assert!(pipeline.entries[..coalescing].iter().all(|entry| matches!(entry.pass, Pass::Rewrite(_))));
        assert!(matches!(pipeline.entries[coalescing + 1].pass, Pass::Build(_)));
    }
}

#[test]
fn only_initially_unlocked_repeaters_without_side_inputs_are_scheduling_only() {
    for initially_locked in [false, true] {
        for side_input in [false, true] {
            let mut graph = fixture(vec![row(4, 0), row(1, 1), row(4, 2)]);
            graph.routes[0].deliveries[2].kind = DeliveryKind::Update {
                direction: Some(BlockFace::South),
            };
            rebuild_links(&mut graph);
            graph.nodes[4].kind = CircuitNodeKind::Electrical(NodeType::Repeater {
                delay: 2,
                facing_diode: false,
            });
            graph.nodes[4].raw_state = Some(
                Block::RedstoneRepeater {
                    repeater: mchprs_blocks::blocks::RedstoneRepeater {
                        facing: BlockDirection::North,
                        delay: 2,
                        powered: false,
                        locked: initially_locked,
                    },
                }
                .get_id(),
            );
            if let ComponentState::Electrical(state) = &mut graph.nodes[4].state {
                state.repeater_locked = initially_locked;
            }
            if side_input {
                graph.power_links.push(PowerLink {
                    to: PowerTarget::Input {
                        node: NodeId::new(4),
                        port: InputPort::Side,
                    },
                    ..graph.power_links[3]
                });
            }
            let mut compact = graph.clone();
            rewrite(&mut compact);
            assert_eq!(compact.routes[0].deliveries.len(), if initially_locked || side_input { 3 } else { 2 });
            let mut general = backend(graph);
            let mut optimized = backend(compact);
            for engine in [&mut general, &mut optimized] {
                engine.deliver_route(RouteId::new(0), None).unwrap();
            }
            same(&general, &optimized);
            for _ in 0..8 {
                general.tick().unwrap();
                optimized.tick().unwrap();
                same(&general, &optimized);
            }
        }
    }
}

#[test]
fn direction_is_ignored_by_diodes_but_remains_meaningful_for_observers() {
    let mut rows = vec![row(1, 0), row(1, 1), row(2, 2), row(2, 3), row(3, 4), row(3, 5), row(3, 6), row(3, 7)];
    rows[1].kind = DeliveryKind::Update {
        direction: Some(BlockFace::South),
    };
    rows[3].kind = DeliveryKind::Update { direction: None };
    rows[4].kind = DeliveryKind::Update {
        direction: Some(BlockFace::South),
    };
    rows[6].kind = DeliveryKind::Update { direction: None };
    let graph = fixture(rows);
    let mut compact = graph.clone();
    rewrite(&mut compact);
    assert_eq!(
        compact.routes[0].deliveries.iter().map(|row| row.sequence).collect::<Vec<_>>(),
        [0, 2, 4, 5, 6]
    );
    let mut general = backend(graph);
    let mut optimized = backend(compact);
    for engine in [&mut general, &mut optimized] {
        engine.deliver_route(RouteId::new(0), None).unwrap();
    }
    assert!(
        optimized.is_pending(NodeId::new(3)),
        "the later facing observer sample must survive a non-facing one"
    );
    same(&general, &optimized);
    for _ in 0..8 {
        general.tick().unwrap();
        optimized.tick().unwrap();
        same(&general, &optimized);
    }
}

#[path = "../../../../../benches/support/cpus.rs"]
#[allow(dead_code)]
mod cpus;

#[test]
#[ignore = "research: prepares both CPUs and measures candidate occurrence coverage; no performance claim"]
fn delayed_update_coalescing_cpu_candidate_census() {
    let worlds = [
        ("chungus", super::chungus_census::load_chungus_world()),
        ("anpu", cpus::load_cpu(*cpus::CPUS.iter().find(|cpu| cpu.name == "anpu_pong").unwrap())),
    ];
    let mut reports = Vec::new();
    for (name, world) in worlds {
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(
            &world,
            world.get_corners(),
            world.scheduler().iter_entries().collect(),
            &monitor,
            AnalysisLimits::for_budget(8),
        )
        .unwrap();
        let options = crate::redpiler::CompilerOptions {
            optimization_level: Some(2),
            budget_multiplier: 8,
            ..Default::default()
        };
        let mut pipeline = passes::default_graph_pipeline(&options);
        pipeline.entries.retain(|entry| matches!(entry.pass, Pass::Rewrite(_)));
        pipeline.entries.push(PassEntry {
            pass: Pass::rewrite(CoalesceDelayedUpdates),
            enabled: true,
        });
        run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
        let statistics = monitor.graph_statistics();
        let counts = statistics
            .preparation_tables
            .iter()
            .filter(|table| table.stage == "experimental delayed updates")
            .map(|table| (table.table.clone(), table.entries))
            .collect::<std::collections::BTreeMap<_, _>>();
        let report = serde_json::json!({"cpu": name, "counts": counts});
        println!("{report}");
        reports.push(report);
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/experimental-delayed-update-census.json");
    std::fs::write(path, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}

#[test]
#[ignore = "expensive: per-tick full CPU state/future comparison of default O4 and O5 plus reset/native continuation"]
fn delayed_update_coalescing_cpu_execution_differential() {
    let ticks = std::env::var("MCHPRS_DELAYED_UPDATE_TICKS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(1024);
    assert!((400..=1024).contains(&ticks));
    let anpu = *cpus::CPUS.iter().find(|cpu| cpu.name == "anpu_pong").unwrap();
    let load = |name| {
        let mut world = if name == "chungus" {
            super::chungus_census::load_chungus_world()
        } else {
            cpus::load_cpu(anpu)
        };
        world.disable_command_output_limits_for_replay();
        world.set_screen_only(false);
        world.flush_block_changes();
        world
    };
    let snapshot = |world: &crate::plot::PlotWorld, tick| {
        serde_json::json!({
            "checkpoint": cpus::checkpoint(world, tick, &[]),
            "ordered_command_output": world.command_output().collect::<Vec<_>>(),
        })
    };
    let mut reports = Vec::new();
    for name in ["chungus", "anpu"] {
        let mut baseline_world = load(name);
        let mut candidate_world = load(name);
        let baseline_options = crate::redpiler::CompilerOptions {
            optimization_level: Some(4),
            budget_multiplier: 8,
            ..Default::default()
        };
        let candidate_options = crate::redpiler::CompilerOptions {
            optimization_level: Some(5),
            budget_multiplier: 8,
            ..Default::default()
        };
        let baseline_pipeline = passes::default_graph_pipeline(&baseline_options);
        let candidate_pipeline = passes::default_graph_pipeline(&candidate_options);
        let baseline_monitor = Arc::new(TaskMonitor::default());
        baseline_monitor.set_budget_multiplier(8);
        let candidate_monitor = Arc::new(TaskMonitor::default());
        candidate_monitor.set_budget_multiplier(8);
        let bounds = baseline_world.get_corners();
        let imported = baseline_world.scheduler().iter_entries().collect::<Vec<_>>();
        let mut baseline = GraphBackend::compile_with_pipeline(
            &baseline_world,
            bounds,
            imported.clone(),
            &baseline_options,
            &baseline_pipeline,
            baseline_monitor,
        )
        .unwrap();
        let mut candidate = GraphBackend::compile_with_pipeline(
            &candidate_world,
            bounds,
            imported,
            &candidate_options,
            &candidate_pipeline,
            candidate_monitor.clone(),
        )
        .unwrap();
        let fixed_ops = |backend: &GraphBackend| backend.fixed_classic.as_ref().map(|program| program.ops.len());
        let rows = |backend: &GraphBackend| backend.program.routes().iter().map(|route| route.deliveries.len()).sum::<usize>();
        let before_ops = fixed_ops(&baseline);
        let after_ops = fixed_ops(&candidate);
        let before_rows = rows(&baseline);
        let after_rows = rows(&candidate);
        let coalesced = candidate_monitor
            .graph_statistics()
            .preparation_tables
            .iter()
            .find(|table| table.stage == "experimental delayed updates" && table.table == "coalesced occurrences")
            .unwrap()
            .entries;
        println!("{name}: coalesced={coalesced}, rows={before_rows}->{after_rows}, fixed_ops={before_ops:?}->{after_ops:?}");
        let start = if name == "chungus" {
            super::chungus_census::START_BUTTON
        } else {
            anpu.origin + anpu.start
        };
        assert!(
            matches!(baseline_world.get_block(start), Block::StoneButton { .. }),
            "real CPU start button"
        );
        baseline.use_block(start).unwrap();
        candidate.use_block(start).unwrap();
        for tick in 0..=ticks {
            if tick > 0 {
                baseline.tick_with_world(&mut baseline_world).unwrap();
                candidate.tick_with_world(&mut candidate_world).unwrap();
            }
            assert!(baseline.state == candidate.state, "{name}: full CircuitState at tick {tick}");
            assert!(
                baseline.future.scheduler.iter().eq(candidate.future.scheduler.iter()),
                "{name}: exact typed scheduler at tick {tick}"
            );
            assert_eq!(
                baseline.future.pending, candidate.future.pending,
                "{name}: pending multiplicity at tick {tick}"
            );
            assert_eq!(
                baseline.future.pending_nodes, candidate.future.pending_nodes,
                "{name}: node pending counts at tick {tick}"
            );
            let clock = |backend: &GraphBackend| {
                (
                    backend.future.clock.logical_tick,
                    backend.future.clock.phase,
                    backend.future.clock.scheduled_advanced,
                )
            };
            assert_eq!(clock(&baseline), clock(&candidate), "{name}: clock at tick {tick}");
            let requests = |backend: &GraphBackend| {
                backend
                    .future
                    .piston_requests
                    .iter()
                    .map(|request| (request.piston, request.action))
                    .collect::<Vec<_>>()
            };
            assert_eq!(requests(&baseline), requests(&candidate), "{name}: piston request FIFO at tick {tick}");
            let deferred = |backend: &GraphBackend| {
                backend
                    .future
                    .deferred_pistons
                    .iter()
                    .map(|event| {
                        (
                            event.piston,
                            event.revision,
                            event.resource.map(|resource| (resource.resource, resource.revision, resource.contact)),
                            event.due_tick,
                            event.extending,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(deferred(&baseline), deferred(&candidate), "{name}: deferred event FIFO at tick {tick}");
            assert_eq!(
                baseline_world.command_output().collect::<Vec<_>>(),
                candidate_world.command_output().collect::<Vec<_>>(),
                "{name}: ordered command output at tick {tick}"
            );
        }
        assert!(
            baseline.counts.scheduled_ticks > 0 && baseline.counts.callbacks > 0,
            "active CPU workload"
        );
        let before_callbacks = baseline.counts.callbacks;
        let after_callbacks = candidate.counts.callbacks;
        baseline.reset(&mut baseline_world);
        candidate.reset(&mut candidate_world);
        assert_eq!(
            snapshot(&baseline_world, ticks),
            snapshot(&candidate_world, ticks),
            "{name}: full reset projection"
        );
        for _ in 0..32 {
            baseline_world.tick_interpreted();
            candidate_world.tick_interpreted();
        }
        assert_eq!(
            snapshot(&baseline_world, ticks + 32),
            snapshot(&candidate_world, ticks + 32),
            "{name}: native continuation"
        );
        reports.push(serde_json::json!({
            "cpu": name, "game_ticks": ticks, "full_state_future_boundaries_matched": ticks + 1, "native_continuation_ticks": 32,
            "coalesced_static_occurrences": coalesced, "delivery_rows_before": before_rows, "delivery_rows_after": after_rows,
            "fixed_ops_before": before_ops, "fixed_ops_after": after_ops,
            "callbacks_before": before_callbacks, "callbacks_after": after_callbacks,
            "comparison": "default O4 versus default O5; no timing claim",
        }));
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/o5-delayed-update-differential.json");
    std::fs::write(path, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}
