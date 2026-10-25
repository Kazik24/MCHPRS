//! Exact admission masks preserve captured dispatch and dynamic guard fallbacks.
use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::passes::{PassContext, planning::PlanBuilder};
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Lever, LeverFace, RedstonePiston};

fn draft(near_contact: bool) -> CompileGraph {
    let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
    world.set_block(
        BlockPos::new(5, 20, 5),
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, mchprs_blocks::BlockDirection::North, false),
        },
    );
    for x in [6, 7] {
        let pos = BlockPos::new(x, 20, 5);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(pos, Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(BlockPos::new(8, 20, 5), Block::RedstoneLamp { lit: false });
    world.set_block(
        BlockPos::new(12, 20, 12),
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFace::East.into(),
                sticky: false,
                extended: false,
            },
        },
    );
    if near_contact {
        world.set_block(BlockPos::new(12, 19, 11), Block::Obsidian {});
        world.set_block(BlockPos::new(12, 20, 11), Block::RedstoneWire { wire: Default::default() });
    }
    passes::rich::prepare(
        &world,
        (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
        vec![],
        &TaskMonitor::default(),
        AnalysisLimits::default(),
    )
    .unwrap()
    .graph
}
fn compile(graph: &mut CompileGraph) {
    let monitor = TaskMonitor::default();
    let options = Default::default();
    let entry = EntryState {
        logical_tick: 0,
        pending_ticks: vec![],
    };
    let context = PassContext::new(&monitor, &options, &entry);
    passes::index_turbo_callbacks::IndexTurboCallbacks.run(graph, &context).unwrap();
    passes::compile_wire_callbacks::CompileWireCallbacks.run(graph, &context).unwrap();
}
fn backend(graph: CompileGraph) -> GraphBackend {
    let (program, state) = graph.lower().unwrap();
    GraphBackend::new_unactivated(program, state, 0).unwrap()
}
fn assert_selections(backend: &GraphBackend) {
    for (index, wire) in backend.program.wires().iter().enumerate() {
        let route = wire.turbo_route.unwrap();
        for (slot, &site) in wire.canonical_neighbors.iter().enumerate() {
            let observed = backend.observe_site(site).unwrap();
            assert_eq!(
                backend.wire_route_admitted(index, route, slot, site, Some(&observed)),
                backend.wire_route_delivery(index, route, slot, site, &observed).is_some(),
                "wire {index} slot {slot}"
            );
        }
    }
}
#[test]
fn wire_callbacks_fixed_masks_ignore_mutable_receiver_power_and_reject_forgery() {
    let mut graph = draft(false);
    compile(&mut graph);
    assert!(
        graph
            .wires
            .iter()
            .all(|wire| wire.execution.callback_selection.unwrap().known == WireCallbackSelection::ALL)
    );
    let mut forged = graph.clone();
    forged.wires[0].execution.callback_selection.as_mut().unwrap().admitted ^= 1;
    assert!(matches!(forged.validate(), Err(GraphBuildError::InvalidWireCallbackSelection { .. })));
    let mut backend = backend(graph);
    assert!(
        backend.program.turbo_callback_indices().is_empty(),
        "fully decided programs need no runtime search index"
    );
    for strength in [0, 1, 15] {
        for state in &mut backend.state.components {
            if let ComponentState::Electrical(state) = state {
                state.output_strength = strength;
                state.powered = strength > 0;
                state.repeater_locked = strength == 1;
            }
        }
        assert_selections(&backend);
    }
    let general = backend.program.without_fixed_wire_callbacks();
    assert_eq!(
        general.turbo_callback_indices().len(),
        general.wires().len(),
        "control restores original grouped search"
    );
}
fn replace_guard(graph: &mut CompileGraph, guard: Guard) -> (usize, usize) {
    let index = graph
        .wires
        .iter()
        .position(|wire| graph.nodes[wire.node.index()].position == Some(BlockPos::new(6, 20, 5)))
        .unwrap();
    let route = graph.wires[index].turbo_route.unwrap();
    let target = graph
        .sites
        .iter()
        .position(|site| site.position == BlockPos::new(7, 20, 5))
        .map(|index| SiteId::new(index as u32))
        .unwrap();
    let slot = graph.wires[index].canonical_neighbors.iter().position(|site| *site == target).unwrap();
    let id = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(guard);
    let rows = graph.routes[route.index()]
        .deliveries
        .iter_mut()
        .filter(|row| row.target == target && row.view == OperandView::Captured && row.receiver.is_some())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    let row = rows.into_iter().next().unwrap();
    row.guard = id;
    let sequence = row.sequence;
    let kind = row.link_kind();
    for link in &mut graph.links {
        if link.callback.route == route && link.callback.sequence == sequence {
            link.kind = kind;
        }
    }
    (index, slot)
}
#[test]
fn wire_callbacks_dynamic_state_keeps_exact_fallback_and_capture_identity() {
    let mut graph = draft(false);
    let (index, slot) = replace_guard(&mut graph, Guard::State(StateTest::PistonExtended(PistonId::new(0))));
    compile(&mut graph);
    assert_eq!(graph.wires[index].execution.callback_selection.unwrap().known & (1 << slot), 0);
    let mut engine = backend(graph);
    for extended in [false, true] {
        engine.state.pistons[0].extended = extended;
        assert_selections(&engine);
    }
    let mut graph = draft(false);
    let other = graph.wires[0].canonical_neighbors[3];
    let (index, slot) = replace_guard(
        &mut graph,
        Guard::Site {
            site: other,
            view: OperandView::Captured,
            test: SiteTest::IsWire,
        },
    );
    compile(&mut graph);
    let plan = graph.wires[index].execution.callback_selection.unwrap();
    assert_ne!(plan.known & (1 << slot), 0);
    assert_eq!(plan.admitted & (1 << slot), 0);
    assert_selections(&backend(graph));
}
#[test]
fn wire_callbacks_contacts_keep_general_selection() {
    let mut graph = draft(true);
    compile(&mut graph);
    let index = graph
        .wires
        .iter()
        .position(|wire| graph.nodes[wire.node.index()].position == Some(BlockPos::new(12, 20, 11)))
        .unwrap();
    assert_ne!(graph.wires[index].execution.callback_selection.unwrap().known, WireCallbackSelection::ALL);
    assert_selections(&backend(graph));
}
