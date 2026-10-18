//! Weighted tracing must preserve channels, attenuation and undelivered live dust.
use super::*;
use crate::plot::PlotWorld;
use crate::world::storage::Chunk;
use mchprs_blocks::{BlockDirection, blocks::RedstoneComparator};
use std::collections::BTreeMap;

const A: BlockPos = BlockPos::new(5, 20, 5);
const B: BlockPos = BlockPos::new(8, 20, 5);
const RECEIVER: BlockPos = BlockPos::new(6, 20, 6);
const WIRES: [BlockPos; 3] = [BlockPos::new(6, 20, 5), BlockPos::new(7, 20, 5), BlockPos::new(7, 20, 6)];

fn fixture() -> CompileGraph {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    for (position, facing) in [(A, BlockDirection::West), (B, BlockDirection::East), (RECEIVER, BlockDirection::North)] {
        world.set_block(position, Block::RedstoneComparator {
            comparator: RedstoneComparator::new(facing, ComparatorMode::Compare, false),
        });
        world.set_block_entity(position, BlockEntity::Comparator { output_strength: 0 });
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    for position in WIRES {
        world.set_block(position, Block::RedstoneWire { wire: Default::default() });
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    for position in WIRES {
        let wire = crate::redstone::wire::get_state_for_placement(&world, position);
        world.set_block(position, Block::RedstoneWire { wire });
    }
    passes::rich::prepare(&world, (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)), vec![],
        &TaskMonitor::default(), AnalysisLimits::default()).unwrap().graph
}

fn backend(graph: CompileGraph) -> GraphBackend {
    let (program, state) = graph.lower().unwrap();
    GraphBackend::new(program, state, 0).unwrap()
}

fn strength(backend: &mut GraphBackend, position: BlockPos, strength: u8) {
    let node = backend.positions[&position];
    match &mut backend.state.components[node.index()] {
        ComponentState::Electrical(state) => { state.output_strength = strength; state.powered = strength != 0; }
        ComponentState::Wire(wire) => wire.power = strength,
        _ => panic!("expected a signal owner"),
    }
}

fn query(backend: &GraphBackend, target: PowerTarget) -> u8 {
    match target {
        PowerTarget::Input { node, port } => backend.input_strength(node, port),
        PowerTarget::Conductor { site, include_dust } => backend.site_power(site, PowerPort::Conducted { include_dust }),
    }
}

#[test]
fn tracer_recovers_multi_ingress_main_and_side_weights() {
    let graph = fixture();
    let plans = infer_fixed_signals(&graph, &TaskMonitor::default()).unwrap();
    let node = |position| NodeId::new(graph.nodes.iter().position(|node| node.position == Some(position)).unwrap() as u32);
    for (port, losses) in [(InputPort::Main, [0, 1]), (InputPort::Side, [2, 1])] {
        let query = plans.traced.iter().find(|query| query.target == PowerTarget::Input { node: node(RECEIVER), port }).unwrap();
        let expected = BTreeMap::from([(node(A), losses[0]), (node(B), losses[1])]);
        let actual = query.inputs.iter().map(|input| { assert!(!input.require_cross); (input.node, input.attenuation) }).collect::<BTreeMap<_, _>>();
        assert_eq!(actual, expected, "{port:?}");
        assert_eq!(query.constant, 0);
    }
}

#[test]
fn compiled_live_queries_preserve_unsettled_analog_wire_state() {
    let graph = fixture();
    let targets = graph.power_links.iter().map(|link| link.to).collect::<std::collections::BTreeSet<_>>();
    let mut compiled = graph.clone();
    compiled.plans.fixed_signals = infer_fixed_signals(&compiled, &TaskMonitor::default()).unwrap();
    assert!(compiled.plans.fixed_signals.fallback.is_empty());
    let mut general = backend(graph);
    let mut compiled = backend(compiled);
    for a in 0..16u8 { for b in 0..16u8 { for old in 0..16u8 {
        for backend in [&mut general, &mut compiled] {
            strength(backend, A, a); strength(backend, B, b);
            for (index, position) in WIRES.into_iter().enumerate() {
                strength(backend, position, (old + index as u8 * 5) % 16);
            }
        }
        for target in &targets { assert_eq!(query(&compiled, *target), query(&general, *target), "{target:?} A={a} B={b} old={old}"); }
        let receiver = compiled.positions[&RECEIVER];
        assert_eq!(compiled.input_strength(receiver, InputPort::Main), old);
        assert_eq!(compiled.input_strength(receiver, InputPort::Side), (old + 10) % 16);
    } } }
}

#[test]
fn validator_rejects_a_well_formed_but_wrong_weight() {
    let mut graph = fixture();
    graph.plans.fixed_signals = infer_fixed_signals(&graph, &TaskMonitor::default()).unwrap();
    graph.validate().unwrap();
    let query = graph.plans.fixed_signals.live.iter_mut().find(|query| !query.inputs.is_empty()).unwrap();
    query.inputs[0].attenuation = (query.inputs[0].attenuation + 1) % 15;
    assert!(matches!(graph.validate(), Err(GraphBuildError::InvalidFixedSignals)));
}
