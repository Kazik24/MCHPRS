use super::*;
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
