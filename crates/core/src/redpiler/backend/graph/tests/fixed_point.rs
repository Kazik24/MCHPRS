use super::*;
use crate::plot::PlotWorld;
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Lever, LeverFace};

#[test]
fn fixed_point_settles_long_lines_from_every_stale_strength() {
    let chunks = (0..2).flat_map(|x| (0..16).map(move |z| Chunk::empty(x, z))).collect();
    let mut world = PlotWorld::from_chunks(0, 0, chunks, Default::default());
    let source = BlockPos::new(2, 10, 8);
    world.set_block(
        source,
        Block::Lever {
            lever: Lever {
                face: LeverFace::Floor,
                ..Default::default()
            },
        },
    );
    world.set_block(source.offset(BlockFace::Bottom), Block::Stone {});
    for x in 3..21 {
        world.set_block(BlockPos::new(x, 10, 8), Block::RedstoneWire { wire: Default::default() });
        world.set_block(BlockPos::new(x, 9, 8), Block::Stone {});
    }
    let graph = passes::rich::prepare(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(31, 15, 15)),
        vec![],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
    .graph;
    for powered in [false, true] {
        for stale in 0..16 {
            let (program, state) = graph.clone().lower().unwrap();
            let mut backend = GraphBackend::new(program, state, 0).unwrap();
            let source_node = backend.positions[&source];
            let ComponentState::Electrical(state) = &mut backend.state.components[source_node.index()] else {
                panic!("lever state");
            };
            state.powered = powered;
            state.output_strength = if powered { 15 } else { 0 };
            let nodes: Vec<_> = (3..21).map(|x| backend.positions[&BlockPos::new(x, 10, 8)]).collect();
            for &node in &nodes {
                let mut wire = backend.wire_state(node);
                wire.power = stale;
                backend.set_wire(node, wire);
            }
            backend.evaluate_fixed_point_wire(nodes[0]).unwrap();
            for (distance, &node) in nodes.iter().enumerate() {
                let expected = if powered { 15u8.saturating_sub(distance as u8) } else { 0 };
                assert_eq!(
                    backend.wire_state(node).power,
                    expected,
                    "powered={powered}, stale={stale}, distance={distance}"
                );
            }
        }
    }
}
