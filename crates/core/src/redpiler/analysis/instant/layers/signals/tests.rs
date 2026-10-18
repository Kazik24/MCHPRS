use super::*;
use crate::redpiler::analysis::instant::layers::{reduction::reduce_roots, tests::empty_graph};
use crate::redpiler::components::{NodeState, NodeType};
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::RedstoneWire;

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
    assert!(error.contains("zero-loss"));
}

#[test]
fn corridor_tracing_checks_cancellation() {
    let monitor = TaskMonitor::default();
    let (graph, internal) = line(3, false);
    let power = index_power(&graph);
    let state = FxHashMap::default();
    let mut builder = LayerBuilder::new(&monitor).unwrap();
    monitor.cancel();
    assert!(
        SignalTracer::new(&graph, &power, &internal, &state, LayerInputRole::RequestSample)
            .query(target(3), 0, &mut builder)
            .unwrap_err()
            .contains("cancel")
    );
}
