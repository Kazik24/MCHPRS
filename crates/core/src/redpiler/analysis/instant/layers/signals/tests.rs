use super::*;
use crate::redpiler::analysis::instant::layers::trace_native_power_targets;
use crate::redpiler::analysis::instant::layers::{reduction::reduce_roots, tests::empty_graph};
use crate::redpiler::components::{NodeState, NodeType};
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::RedstoneWire;

fn profile(block: Block, electrical: ProfileElectrical) -> Profile {
    Profile {
        raw_state: block.get_id(),
        registry_type: 0,
        solid: block.is_solid(),
        transparent: block.is_transparent(),
        cube: block.is_cube(),
        electrical,
    }
}

fn target(node: u32) -> PowerTarget {
    PowerTarget::Input {
        node: NodeId::new(node),
        port: InputPort::Main,
    }
}

fn link(from: u32, to: u32, attenuation: u8, guard: u32) -> PowerLink {
    PowerLink {
        from: SiteId::new(from),
        from_port: PowerPort::Output,
        to: target(to),
        face: BlockFace::West,
        attenuation,
        guard: GuardId::new(guard),
    }
}

fn line(length: u32, bidirectional: bool) -> (CompileGraph, FxHashSet<NodeId>) {
    let mut graph = empty_graph(link(0, 1, 0, 0));
    graph.power_links.clear();
    let mut internal = FxHashSet::default();
    for index in 0..=length {
        let is_wire = index != 0;
        let node = NodeId::new(index);
        if is_wire {
            internal.insert(node);
        }
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(index as i32, 20, 0)),
            raw_state: Some(Block::Stone {}.get_id()),
            kind: if is_wire {
                CircuitNodeKind::Wire(index as usize - 1)
            } else {
                CircuitNodeKind::Electrical(NodeType::Constant)
            },
            state: if is_wire {
                ComponentState::Wire(RedstoneWire::default())
            } else {
                ComponentState::Electrical(NodeState::default())
            },
            output_route: None,
        });
        graph.sites.push(SiteSpec {
            position: BlockPos::new(index as i32, 20, 0),
            source: SiteSource::Component(node),
            source_strength: 0,
        });
        if is_wire {
            graph.power_links.push(link(index - 1, index, if index == 1 { 0 } else { 1 }, 0));
        }
        if bidirectional && index > 1 {
            graph.power_links.push(link(index, index - 1, 1, 0));
        }
    }
    (graph, internal)
}

fn evaluate(builder: &LayerBuilder<'_>, root: LayerExprId, mut input: impl FnMut(LayerInput) -> bool) -> bool {
    let program = reduce_roots(
        &builder.inputs,
        &builder.expressions,
        vec![(LayerRoot::Output(0), root)],
        &TaskMonitor::default(),
    )
    .unwrap();
    let mut scratch = vec![false; program.expressions.len()];
    program.evaluate(&mut scratch, &mut input).unwrap();
    scratch[program.roots[0].1.0 as usize]
}

#[test]
fn logical_equation_wire_support_maps_raw_queries_and_keeps_only_live_roots() {
    let mut graph = empty_graph(link(0, 1, 0, 0));
    for index in 0..2 {
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(index, 20, 0)),
            raw_state: Some(
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                }
                .get_id(),
            ),
            kind: CircuitNodeKind::Wire(index as usize),
            state: ComponentState::Wire(RedstoneWire::default()),
            output_route: None,
        });
    }
    let first = NodeId::new(0);
    let second = NodeId::new(1);
    let first_raw = LayerExprId(3);
    let second_raw = LayerExprId(4);
    let first_expanded = LayerExprId(13);
    let second_expanded = LayerExprId(14);
    let mut memo = SignalMemo::default();
    memo.targets.insert((target(0), 0), first_raw);
    memo.targets.insert((target(1), 0), second_raw);
    let expanded = [(first_raw, first_expanded), (second_raw, second_expanded)].into_iter().collect();
    let live_nonconstant = [first_expanded].into_iter().collect();

    assert_eq!(
        memo.live_expanded_wire_nodes(&graph, &expanded, &live_nonconstant),
        [first].into_iter().collect(),
        "wire support must follow raw query roots through expansion and exclude dead roots"
    );
    assert!(!memo.live_expanded_wire_nodes(&graph, &expanded, &FxHashSet::default()).contains(&second));
}

#[test]
fn corridor_thresholds_match_all_strengths_losses_and_positive_loss_cycles() {
    let monitor = TaskMonitor::default();
    for bidirectional in [false, true] {
        for length in 1..=18 {
            let (graph, internal) = line(length, bidirectional);
            let power = index_power(&graph);
            for loss in 0..=15 {
                let mut builder = LayerBuilder::new(&monitor).unwrap();
                let state = FxHashMap::default();
                let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample);
                let root = tracer.query(target(length), loss, &mut builder).unwrap();
                for strength in 0u8..=15 {
                    assert_eq!(
                        evaluate(&builder, root, |input| match input.source {
                            LayerInputSource::SourceAbove {
                                site,
                                port: PowerPort::Output,
                                threshold,
                            } => {
                                assert_eq!(site, SiteId::new(0), "owned wires cannot become inputs");
                                strength > threshold
                            }
                            other => panic!("unexpected input {other:?}"),
                        }),
                        strength.saturating_sub((length - 1) as u8).saturating_sub(loss) > 0,
                        "length={length}, loss={loss}, strength={strength}, cycle={bidirectional}"
                    );
                }
            }
        }
    }
}

#[test]
fn completed_signal_memo_reuses_queries_without_changing_roots() {
    let monitor = TaskMonitor::default();
    let (graph, internal) = line(8, false);
    let power = index_power(&graph);
    let state = FxHashMap::default();

    let mut memo_builder = LayerBuilder::new(&monitor).unwrap();
    let mut memo = SignalMemo::default();
    let mut memo_roots = Vec::new();
    let mut memo_work = Vec::new();
    for _ in 0..2 {
        let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample).with_memo(memo);
        memo_roots.push(tracer.query(target(8), 0, &mut memo_builder).unwrap());
        memo = tracer.into_memo();
        memo_work.push(memo_builder.budget.work);
    }

    let mut fresh_builder = LayerBuilder::new(&monitor).unwrap();
    let mut fresh_roots = Vec::new();
    let mut fresh_work = Vec::new();
    for _ in 0..2 {
        let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample);
        fresh_roots.push(tracer.query(target(8), 0, &mut fresh_builder).unwrap());
        fresh_work.push(fresh_builder.budget.work);
    }

    assert_eq!(memo_roots[0], memo_roots[1]);
    assert_eq!(memo_roots, fresh_roots);
    assert!(memo_work[1] - memo_work[0] < fresh_work[1] - fresh_work[0]);
}

#[test]
fn native_power_trace_does_not_retain_a_dead_wire_through_an_aliased_query_root() {
    let monitor = TaskMonitor::default();
    let mut graph = empty_graph(link(0, 0, 0, 0));
    graph.power_links.clear();

    let mut add_component = |position: i32, kind: CircuitNodeKind, block: Block| {
        let node = NodeId::new(graph.nodes.len() as u32);
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(position, 20, 0)),
            raw_state: Some(block.get_id()),
            state: match &kind {
                CircuitNodeKind::Wire(_) => ComponentState::Wire(RedstoneWire::default()),
                _ => ComponentState::Electrical(NodeState::default()),
            },
            kind,
            output_route: None,
        });
        let site = SiteId::new(graph.sites.len() as u32);
        graph.sites.push(SiteSpec {
            position: BlockPos::new(position, 20, 0),
            source: SiteSource::Component(node),
            source_strength: 15,
        });
        (node, site)
    };

    let wire_block = Block::RedstoneWire {
        wire: RedstoneWire::default(),
    };
    let (wire, wire_site) = add_component(0, CircuitNodeKind::Wire(0), wire_block);
    let (_, first_sink) = add_component(1, CircuitNodeKind::Electrical(NodeType::Constant), Block::Stone {});
    let (_, second_sink) = add_component(2, CircuitNodeKind::Electrical(NodeType::Constant), Block::Stone {});
    let (_, lever_site) = add_component(
        3,
        CircuitNodeKind::Electrical(NodeType::Lever),
        Block::Lever { lever: Default::default() },
    );
    graph.profiles.push(profile(Block::RedstoneBlock {}, ProfileElectrical::RedstoneSource));
    let literal_site = SiteId::new(graph.sites.len() as u32);
    graph.sites.push(SiteSpec {
        position: BlockPos::new(4, 20, 0),
        source: SiteSource::Constant(ProfileId::new(0)),
        source_strength: 15,
    });

    graph.power_links.extend([
        link(lever_site.index() as u32, wire.index() as u32, 0, 0),
        link(wire_site.index() as u32, first_sink.index() as u32, 0, 0),
        link(literal_site.index() as u32, first_sink.index() as u32, 0, 0),
        link(lever_site.index() as u32, second_sink.index() as u32, 0, 0),
    ]);

    let trace = trace_native_power_targets(&graph, &[target(first_sink.index() as u32), target(second_sink.index() as u32)], &monitor).unwrap();

    assert!(trace.inputs.contains(&LayerInputSource::SourceAbove {
        site: lever_site,
        port: PowerPort::Output,
        threshold: 0,
    }));
    assert!(!trace.wires.contains(&wire), "wireA only contributes to the first, constant-true query");
    assert!(
        !trace.source_sites.contains(&wire_site),
        "wireA's aliased equation root is not a live source site"
    );
}

#[test]
fn failed_query_does_not_export_visiting_entries_in_signal_memo() {
    let monitor = TaskMonitor::default();
    let (mut graph, internal) = line(0, false);
    graph.power_links = vec![PowerLink {
        from: SiteId::new(0),
        from_port: PowerPort::Conducted { include_dust: true },
        to: PowerTarget::Conductor {
            site: SiteId::new(0),
            include_dust: true,
        },
        ..link(0, 0, 0, 0)
    }];
    let power = index_power(&graph);
    let state = FxHashMap::default();
    let query = PowerTarget::Conductor {
        site: SiteId::new(0),
        include_dust: true,
    };
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample);
    assert!(tracer.query(query, 0, &mut builder).is_err());

    let memo = tracer.into_memo();
    assert!(memo.targets.is_empty());
    assert!(memo.sources.is_empty());

    let mut retry = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample).with_memo(memo);
    assert!(matches!(retry.query(query, 0, &mut builder), Err(ConstructionError::Invalid(message)) if message.contains("zero-loss")));
}

#[test]
fn disabled_transfer_is_used_only_for_positive_all_guard_atoms() {
    let monitor = TaskMonitor::default();
    let (mut graph, internal) = line(1, false);
    let resource = ResourceId::new(7);
    let destination = ContactId::new(9);
    graph.guards.extend([
        Guard::State(StateTest::ResourceAt {
            resource,
            contact: destination,
        }),
        Guard::State(StateTest::ResourceEnabled(resource)),
        Guard::All(vec![GuardId::new(1), GuardId::new(2)].into()),
        Guard::All(vec![GuardId::new(3)].into()),
        Guard::Any(vec![GuardId::new(1), GuardId::new(2)].into()),
        Guard::Not(GuardId::new(1)),
        Guard::All(vec![GuardId::new(5), GuardId::new(2)].into()),
        Guard::State(StateTest::ResourceAt {
            resource,
            contact: ContactId::new(10),
        }),
        Guard::All(vec![GuardId::new(8), GuardId::new(2)].into()),
    ]);
    let power = index_power(&graph);
    let state = FxHashMap::default();
    let mut disabled = FxHashSet::default();
    disabled.insert((resource, destination));
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample).with_disabled_destinations(&disabled);

    let positive_all = tracer.guard(GuardId::new(4), &mut builder).unwrap();
    let disjunction = tracer.guard(GuardId::new(5), &mut builder).unwrap();
    let negated_atom = tracer.guard(GuardId::new(7), &mut builder).unwrap();
    let different_contact = tracer.guard(GuardId::new(9), &mut builder).unwrap();

    // The contracted Near token is disabled at a settled cut, so the direct
    // positive ResourceAt && ResourceEnabled conjunction cannot hold.
    assert!(!evaluate(&builder, positive_all, |_| panic!("folded guard has no inputs")));
    // Disjunction and negation are deliberately not interpreted as positive facts.
    assert!(evaluate(&builder, disjunction, |input| match input.source {
        LayerInputSource::State(StateTest::ResourceAt { .. }) | LayerInputSource::State(StateTest::ResourceEnabled(_)) => true,
        other => panic!("unexpected input {other:?}"),
    }));
    assert!(evaluate(&builder, negated_atom, |input| match input.source {
        LayerInputSource::State(StateTest::ResourceAt { .. }) => false,
        LayerInputSource::State(StateTest::ResourceEnabled(_)) => true,
        other => panic!("unexpected input {other:?}"),
    }));
    assert!(evaluate(&builder, different_contact, |_| true));

    // Other tracer callers carry no cell-transfer contract and retain the raw guard.
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample);
    let raw = tracer.guard(GuardId::new(4), &mut builder).unwrap();
    assert!(evaluate(&builder, raw, |_| true));
}

#[test]
fn conditional_contacts_and_prefix_writes_are_substituted_not_sampled_as_wires() {
    let monitor = TaskMonitor::default();
    let (mut graph, internal) = line(3, false);
    let enabled = StateTest::ResourceEnabled(ResourceId::new(0));
    graph.guards.push(Guard::State(enabled));
    graph.power_links[0].guard = GuardId::new(1);
    let power = index_power(&graph);
    for prefix in [None, Some(false), Some(true)] {
        let mut builder = LayerBuilder::new(&monitor).unwrap();
        let mut state = FxHashMap::default();
        if let Some(value) = prefix {
            state.insert(enabled, builder.constant(value).unwrap());
        }
        let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::AcceptanceRevalidation);
        let root = tracer.query(target(3), 0, &mut builder).unwrap();
        for selected in [false, true] {
            for strength in 0u8..=15 {
                assert_eq!(
                    evaluate(&builder, root, |input| match input.source {
                        LayerInputSource::SourceAbove { threshold, .. } => strength > threshold,
                        LayerInputSource::State(test) => {
                            assert_eq!(test, enabled);
                            selected
                        }
                        other => panic!("unexpected {other:?}"),
                    }),
                    prefix.unwrap_or(selected) && strength > 2
                );
            }
        }
    }
}

#[test]
fn zero_source_wire_cycles_terminate_but_zero_loss_conductor_cycles_decline() {
    let monitor = TaskMonitor::default();
    let (mut graph, internal) = line(3, true);
    graph.power_links.remove(0);
    let power = index_power(&graph);
    let state = FxHashMap::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let root = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample)
        .query(target(3), 0, &mut builder)
        .unwrap();
    assert!(!evaluate(&builder, root, |_| panic!("a closed unpowered loop has no input")));
    graph.power_links = vec![PowerLink {
        from: SiteId::new(0),
        from_port: PowerPort::Conducted { include_dust: true },
        to: PowerTarget::Conductor {
            site: SiteId::new(0),
            include_dust: true,
        },
        ..link(0, 0, 0, 0)
    }];
    let power = index_power(&graph);
    let error = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample)
        .query(
            PowerTarget::Conductor {
                site: SiteId::new(0),
                include_dust: true,
            },
            0,
            &mut builder,
        )
        .unwrap_err();
    assert!(matches!(error, ConstructionError::Invalid(message) if message.contains("zero-loss")));
}

#[test]
fn corridor_tracing_checks_cancellation() {
    let monitor = TaskMonitor::default();
    let (graph, internal) = line(3, false);
    let power = index_power(&graph);
    let state = FxHashMap::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    monitor.cancel();
    assert!(matches!(
        SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample)
            .query(target(3), 0, &mut builder),
        Err(ConstructionError::Invalid(message)) if message.contains("cancel")
    ));
}

#[test]
fn contact_power_and_geometry_use_the_first_selected_profile() {
    let monitor = TaskMonitor::default();
    let (mut graph, internal) = line(1, false);
    let selected = StateTest::ResourceEnabled(ResourceId::new(0));
    graph.guards.push(Guard::State(selected));
    graph.profiles = vec![
        profile(Block::Air, ProfileElectrical::None),
        profile(Block::RedstoneBlock, ProfileElectrical::RedstoneSource),
    ];
    graph.sites[0].source = SiteSource::Contact(ContactId::new(0));
    graph.contacts.push(ContactSpec {
        node: NodeId::new(0),
        position: BlockPos::new(0, 20, 0),
        variants: vec![
            ContactVariant {
                when: GuardId::new(1),
                profile: ProfileId::new(0),
                receiver: None,
            },
            ContactVariant {
                when: GuardId::new(0),
                profile: ProfileId::new(1),
                receiver: None,
            },
        ]
        .into(),
    });
    let power = index_power(&graph);
    let state = FxHashMap::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample);
    let powered = tracer.query(target(1), 0, &mut builder).unwrap();
    let redstone_profile = tracer
        .site_test(SiteId::new(0), SiteTest::Profile(ProfileId::new(1)), &mut builder)
        .unwrap();
    for first in [false, true] {
        for root in [powered, redstone_profile] {
            assert_eq!(
                evaluate(&builder, root, |input| {
                    assert_eq!(input.source, LayerInputSource::State(selected));
                    first
                }),
                !first,
                "a later true variant must not override an earlier selected Air profile"
            );
        }
    }
    // Graph uses the profile's electrical classification, not the raw material alone.
    graph.profiles[1].electrical = ProfileElectrical::None;
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let root = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample)
        .query(target(1), 0, &mut builder)
        .unwrap();
    assert!(!evaluate(&builder, root, |_| false));
    graph.profiles[1].electrical = ProfileElectrical::Component;
    assert!(matches!(
        SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample)
            .query(target(1), 0, &mut builder),
        Err(ConstructionError::Invalid(message)) if message.contains("active contact")
    ));
}

#[test]
fn native_power_trace_keeps_only_inputs_reachable_from_each_query_root() {
    let monitor = TaskMonitor::default();
    let (mut graph, _) = line(1, false);
    let resource = ResourceId::new(0);
    graph.profiles = vec![
        profile(Block::Air, ProfileElectrical::None),
        profile(Block::RedstoneBlock, ProfileElectrical::RedstoneSource),
    ];
    graph.guards.push(Guard::State(StateTest::ResourceEnabled(resource)));
    graph.sites[0].source = SiteSource::Contact(ContactId::new(0));
    graph.contacts.push(ContactSpec {
        node: NodeId::new(0),
        position: BlockPos::new(0, 20, 0),
        variants: vec![ContactVariant {
            when: GuardId::new(1),
            profile: ProfileId::new(0),
            receiver: None,
        }]
        .into(),
    });
    graph.power_links[0].guard = GuardId::new(1);

    let mut add_node = |position: i32, kind: CircuitNodeKind| {
        let node = NodeId::new(graph.nodes.len() as u32);
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(position, 20, 0)),
            raw_state: Some(Block::RedstoneBlock {}.get_id()),
            kind,
            state: ComponentState::Electrical(NodeState::default()),
            output_route: None,
        });
        let site = SiteId::new(graph.sites.len() as u32);
        graph.sites.push(SiteSpec {
            position: BlockPos::new(position, 20, 0),
            source: SiteSource::Component(node),
            source_strength: 15,
        });
        (node, site)
    };
    let (_lever, lever_site) = add_node(2, CircuitNodeKind::Electrical(NodeType::Lever));
    let (observer, observer_site) = add_node(
        3,
        CircuitNodeKind::Observer {
            watched: SiteId::new(0),
            facing: BlockFace::North,
        },
    );
    let (live_target, _) = add_node(4, CircuitNodeKind::Electrical(NodeType::Constant));
    graph.power_links.push(link(lever_site.index() as u32, live_target.index() as u32, 0, 0));
    graph
        .power_links
        .push(link(observer_site.index() as u32, live_target.index() as u32, 0, 0));

    let trace = trace_native_power_targets(&graph, &[target(1), target(live_target.index() as u32)], &monitor).unwrap();
    assert!(trace.inputs.contains(&LayerInputSource::SourceAbove {
        site: lever_site,
        port: PowerPort::Output,
        threshold: 0,
    }));
    assert!(trace.inputs.contains(&LayerInputSource::SourceAbove {
        site: observer_site,
        port: PowerPort::Output,
        threshold: 0,
    }));
    assert!(!trace.inputs.contains(&LayerInputSource::State(StateTest::ResourceEnabled(resource))));
    assert!(!trace.source_sites.contains(&SiteId::new(0)));
    assert!(trace.source_sites.contains(&lever_site));
    assert!(trace.source_sites.contains(&observer_site));
    assert!(!trace.wires.contains(&NodeId::new(1)));
    assert_eq!(observer, NodeId::new(3));
}

#[test]
fn symbolic_wire_directions_match_native_regulation_in_all_local_shape_combinations() {
    use crate::redstone::wire::{get_current_side, get_raw_sides_from_neighbors, regulate_sides};
    use mchprs_blocks::blocks::RedstoneWireSide;
    let monitor = TaskMonitor::default();
    let saved = RedstoneWire::new(
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        15,
    );
    // Each side can be disconnected, connected, blocked, ascending or descending.
    for ceiling in [Block::Air, Block::Stone {}] {
        for mut shape in 0..6usize.pow(4) {
            let mut neighbors = [Block::Air; 24];
            neighbors[3] = ceiling;
            for (near, above, below) in [(4, 20, 17), (5, 21, 18), (1, 13, 12), (0, 8, 7)] {
                let configuration = shape % 6;
                shape /= 6;
                match configuration {
                    0 => {}
                    1 => neighbors[near] = Block::RedstoneWire { wire: saved },
                    2 => neighbors[near] = Block::Stone {},
                    3 => neighbors[above] = Block::RedstoneWire { wire: saved },
                    4 => neighbors[below] = Block::RedstoneWire { wire: saved },
                    5 => {
                        neighbors[near] = Block::Stone {};
                        neighbors[above] = Block::RedstoneWire { wire: saved };
                        neighbors[below] = Block::RedstoneWire { wire: saved };
                    }
                    _ => unreachable!(),
                }
            }
            let (mut graph, _) = line(0, false);
            graph.nodes[0].kind = CircuitNodeKind::Wire(0);
            graph.nodes[0].state = ComponentState::Wire(saved);
            graph.profiles = neighbors.iter().map(|&block| profile(block, ProfileElectrical::None)).collect();
            graph.sites = (0..24)
                .map(|i| SiteSpec {
                    position: BlockPos::new(i, 20, 0),
                    source: SiteSource::Constant(ProfileId::new(i as u32)),
                    source_strength: 0,
                })
                .collect();
            graph.wires.push(WirePlan {
                node: NodeId::new(0),
                canonical_neighbors: std::array::from_fn(|i| SiteId::new(i as u32)),
                shape_route: None,
                turbo_route: None,
                execution: Default::default(),
            });
            let native = regulate_sides(saved, get_raw_sides_from_neighbors(saved, &neighbors));
            let power = index_power(&graph);
            let internal = FxHashSet::default();
            let state = FxHashMap::default();
            let mut builder = LayerBuilder::new(&monitor).unwrap();
            let mut tracer = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample);
            for face in BlockFace::values() {
                let expected = match face {
                    BlockFace::Top => true,
                    BlockFace::Bottom => false,
                    _ => !get_current_side(native, face.unwrap_direction().opposite()).is_none(),
                };
                for port in [PowerPort::Weak(face), PowerPort::Strong(face)] {
                    let root = tracer.wire_emits(NodeId::new(0), port, &mut builder).unwrap();
                    assert_eq!(
                        evaluate(&builder, root, |_| panic!("fixed geometry has no symbolic inputs")),
                        expected,
                        "{face:?}, {neighbors:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn stable_wire_mode_honors_saved_dot_and_cross_without_weakening_legacy_queries() {
    use mchprs_blocks::blocks::RedstoneWireSide;

    let cross = RedstoneWire::new(
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        RedstoneWireSide::Side,
        15,
    );
    for (saved, east_connection) in [(RedstoneWire::default(), false), (RedstoneWire::default(), true), (cross, false)] {
        let monitor = TaskMonitor::default();
        let (mut graph, _) = line(0, false);
        graph.nodes[0].kind = CircuitNodeKind::Wire(0);
        graph.nodes[0].state = ComponentState::Wire(saved);
        graph.profiles = vec![profile(Block::Air, ProfileElectrical::None); 24];
        if east_connection {
            graph.profiles[1] = profile(Block::RedstoneBlock, ProfileElectrical::RedstoneSource);
        }
        graph.sites = (0..24)
            .map(|index| SiteSpec {
                position: BlockPos::new(index, 20, 0),
                source: SiteSource::Constant(ProfileId::new(index as u32)),
                source_strength: 0,
            })
            .collect();
        graph.wires.push(WirePlan {
            node: NodeId::new(0),
            canonical_neighbors: std::array::from_fn(|index| SiteId::new(index as u32)),
            shape_route: None,
            turbo_route: None,
            execution: Default::default(),
        });
        let power = index_power(&graph);
        let internal = FxHashSet::default();
        let state = FxHashMap::default();
        let mut legacy_builder = LayerBuilder::new(&monitor).unwrap();
        let legacy = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample).wire_emits(
            NodeId::new(0),
            PowerPort::Weak(BlockFace::West),
            &mut legacy_builder,
        );
        if crate::redstone::wire::is_dot(saved) {
            assert!(matches!(legacy, Err(ConstructionError::Unsupported(message)) if message.contains("dot/cross")));
        }
        let mut stable_builder = LayerBuilder::new(&monitor).unwrap();
        let root = SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample)
            .with_stable_wire_modes()
            .wire_emits(NodeId::new(0), PowerPort::Weak(BlockFace::West), &mut stable_builder)
            .unwrap();
        let modes: &[bool] = if crate::redstone::wire::is_dot(saved) {
            &[true, false]
        } else {
            &[false]
        };
        for &dot_mode in modes {
            let expected = if dot_mode { east_connection } else { true };
            assert_eq!(
                evaluate(&stable_builder, root, |input| match input.source {
                    LayerInputSource::WireDot { node } => {
                        assert_eq!(node, NodeId::new(0));
                        dot_mode
                    }
                    _ => unreachable!("geometry is fixed in this fixture"),
                }),
                expected,
                "saved={saved:?}, east_connection={east_connection}, live_dot={dot_mode}"
            );
        }
    }

    let monitor = TaskMonitor::default();
    let (mut graph, _) = line(0, false);
    graph.nodes[0].kind = CircuitNodeKind::Wire(0);
    graph.nodes[0].state = ComponentState::Wire(RedstoneWire::default());
    graph.profiles = vec![
        profile(Block::Air, ProfileElectrical::None),
        profile(Block::RedstoneBlock, ProfileElectrical::RedstoneSource),
    ];
    graph.sites = (0..24)
        .map(|index| SiteSpec {
            position: BlockPos::new(index, 20, 0),
            source: SiteSource::Constant(ProfileId::new(0)),
            source_strength: 0,
        })
        .collect();
    graph.guards = vec![
        Guard::Always,
        Guard::State(StateTest::PistonExtended(PistonId::new(0))),
        Guard::Not(GuardId::new(1)),
    ];
    graph.contacts.push(ContactSpec {
        node: NodeId::new(0),
        position: BlockPos::new(1, 20, 0),
        variants: vec![
            ContactVariant {
                when: GuardId::new(1),
                profile: ProfileId::new(1),
                receiver: None,
            },
            ContactVariant {
                when: GuardId::new(2),
                profile: ProfileId::new(0),
                receiver: None,
            },
        ]
        .into_boxed_slice(),
    });
    graph.sites[1].source = SiteSource::Contact(ContactId::new(0));
    graph.wires.push(WirePlan {
        node: NodeId::new(0),
        canonical_neighbors: std::array::from_fn(|index| SiteId::new(index as u32)),
        shape_route: None,
        turbo_route: None,
        execution: Default::default(),
    });
    let power = index_power(&graph);
    let state = FxHashMap::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    let root = SignalTracer::new(&graph, &power, &FxHashSet::default(), &state, LayerInputRole::RequestSample)
        .with_stable_wire_modes()
        .wire_emits(NodeId::new(0), PowerPort::Weak(BlockFace::West), &mut builder)
        .unwrap();
    let dot = RedstoneWire::default();
    let connected = crate::redstone::wire::on_neighbor_changed_from(
        dot,
        RedstoneWire {
            east: RedstoneWireSide::Side,
            ..Default::default()
        },
        BlockFace::West,
    );
    let disconnected = crate::redstone::wire::on_neighbor_changed_from(connected, RedstoneWire::default(), BlockFace::West);
    assert!(crate::redstone::wire::is_cross(disconnected));
    for (native, neighbor_connected) in [(dot, false), (connected, true), (disconnected, false)] {
        let expected = crate::redstone::wire::get_current_side(native, BlockDirection::East) != RedstoneWireSide::None;
        let actual = evaluate(&builder, root, |input| match input.source {
            LayerInputSource::WireDot { node } => {
                assert_eq!(node, NodeId::new(0));
                crate::redstone::wire::is_dot(native)
            }
            LayerInputSource::State(StateTest::PistonExtended(piston)) if piston == PistonId::new(0) => neighbor_connected,
            source => panic!("unexpected wire equation input: {source:?}"),
        });
        assert_eq!(actual, expected, "native wire={native:?}, connected={neighbor_connected}");
    }
}
