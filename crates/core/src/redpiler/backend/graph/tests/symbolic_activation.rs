//! Control-edge admission must retain stationary controls and decline pending releases.
use super::*;
use crate::redpiler::analysis::instant_regions::compiler::aot;
use mchprs_blocks::BlockDirection;
use mchprs_blocks::blocks::{ButtonFace, Lever, StoneButton};

#[test]
fn symbolic_control_edges_preserve_buttons_and_pending_release_handoff() {
    for powered_before in [false, true] {
        for pending_release in [false, true] {
            verify_control_edge(powered_before, pending_release);
        }
    }
}

#[test]
fn captured_shape_admission_rejects_changed_histories_and_preserves_interpreter_handoff() {
    use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};
    for changed_shape in [false, true] {
        let trigger = ORIGIN + BlockPos::new(2, 1, 2);
        let first = trigger.offset(BlockFace::East);
        let second = first.offset(BlockFace::East);
        let lamp = second.offset(BlockFace::East);
        let bounds = (ORIGIN, ORIGIN + BlockPos::new(8, 3, 5));
        let mut native = empty_world();
        for position in [trigger, first, second] {
            native.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        }
        native.set_block(
            trigger,
            Block::Lever {
                lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
            },
        );
        let shape = RedstoneWire {
            north: RedstoneWireSide::Side,
            south: RedstoneWireSide::Side,
            east: RedstoneWireSide::Side,
            west: RedstoneWireSide::Side,
            power: 0,
        };
        native.set_block(first, Block::RedstoneWire { wire: shape });
        native.set_block(second, Block::RedstoneWire { wire: shape });
        native.set_block(lamp, Block::RedstoneLamp { lit: false });
        let monitor = TaskMonitor::default();
        let mut graph = passes::rich::prepare(&native, bounds, vec![], &monitor, AnalysisLimits::default())
            .unwrap()
            .graph;
        let control = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(trigger)).unwrap() as u32);
        let kernel = Arc::new(
            aot::compile_for_entry(
                &graph,
                control,
                true,
                crate::redpiler::analysis::instant_regions::compiler::entry::ShapeEntry::Captured,
                &monitor,
            )
            .unwrap(),
        );
        let (program, state) = graph.clone().lower().unwrap();
        let mut exact = GraphBackend::new(program, state, 0).unwrap();
        graph.plans.instant.symbolic_kernels.push(kernel.clone());
        let (program, state) = graph.lower().unwrap();
        let mut compiled = GraphBackend::new(program, state, 0).unwrap();
        if changed_shape {
            let changed = RedstoneWire {
                north: RedstoneWireSide::None,
                south: RedstoneWireSide::None,
                ..shape
            };
            for position in [first, second] {
                native.set_block(position, Block::RedstoneWire { wire: changed });
                for backend in [&mut exact, &mut compiled] {
                    let node = backend.positions[&position];
                    backend.state.components[node.index()] = ComponentState::Wire(changed);
                }
            }
        }
        assert!(
            kernel
                .inputs
                .iter()
                .all(|input| input.read(&compiled.program, &compiled.state, &kernel.shapes).is_some())
        );
        assert_eq!(compiled.prepare_symbolic_kernel(control).is_some(), !changed_shape);
        let positions = [trigger, first, second, lamp];
        let mut scratch = empty_world();
        native_step(
            &mut native,
            Step::SetLever {
                position: trigger,
                powered: true,
            },
        );
        exact.use_block(trigger).unwrap();
        compiled.use_block(trigger).unwrap();
        assert_eq!(compiled.future.symbolic_cursor.is_some(), !changed_shape);
        for _ in 0..8 {
            assert_eq!(boundary(&compiled), boundary(&exact));
            check_boundary(&compiled, &native, &mut scratch, &positions, "shape admission").unwrap();
            native.tick_interpreted();
            exact.tick().unwrap();
            compiled.tick().unwrap();
        }
        assert_eq!(compiled.execution_counts().wire_walks > 0, changed_shape);
        compiled.reset(&mut scratch);
        for _ in 0..4 {
            scratch.tick_interpreted();
            native.tick_interpreted();
            assert_eq!(future(&scratch, &positions), future(&native, &positions));
        }
    }
}

fn verify_control_edge(powered_before: bool, pending_release: bool) {
    let trigger = ORIGIN + BlockPos::new(2, 1, 2);
    let lamp = trigger.offset(BlockFace::East);
    let button = ORIGIN + BlockPos::new(8, 1, 8);
    let bounds = (ORIGIN, ORIGIN + BlockPos::new(12, 3, 12));
    let mut native = empty_world();
    native.set_block(trigger.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(
        trigger,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, powered_before),
        },
    );
    native.set_block(lamp, Block::RedstoneLamp { lit: powered_before });
    native.set_block(button.offset(BlockFace::Bottom), Block::Stone {});
    native.set_block(
        button,
        Block::StoneButton {
            button: StoneButton::new(ButtonFace::Floor, BlockDirection::North, true),
        },
    );
    let monitor = TaskMonitor::default();
    let prepared = passes::rich::prepare(&native, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    let mut graph = prepared.graph;
    let control = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(trigger)).unwrap() as u32);
    let kernel = aot::compile_candidate(&graph, control, !powered_before, &monitor).unwrap();
    assert_eq!(kernel.powered_after, !powered_before);
    assert!(kernel.estimated_heap_bytes() > kernel.plan.estimated_heap_bytes());
    let (program, state) = graph.clone().lower().unwrap();
    let mut exact = GraphBackend::new(program, state, 0).unwrap();
    graph.plans.instant.symbolic_kernels = vec![Arc::new(kernel)];
    let (program, state) = graph.lower().unwrap();
    let mut compiled = GraphBackend::new(program, state, 0).unwrap();
    if pending_release {
        native.schedule_tick(button, 2, TickPriority::Normal);
        exact.schedule(exact.positions[&button], 2, TickPriority::Normal);
        compiled.schedule(compiled.positions[&button], 2, TickPriority::Normal);
    }
    let mut scratch = empty_world();
    let positions = [trigger, lamp, button];
    native_step(
        &mut native,
        Step::SetLever {
            position: trigger,
            powered: !powered_before,
        },
    );
    exact.use_block(trigger).unwrap();
    compiled.use_block(trigger).unwrap();
    assert_eq!(compiled.future.symbolic_cursor.is_some(), !pending_release);
    for tick in 0..8 {
        assert_eq!(boundary(&compiled), boundary(&exact), "complete future at tick {tick}");
        check_boundary(&compiled, &native, &mut scratch, &positions, "control edge").unwrap();
        native.tick_interpreted();
        exact.tick().unwrap();
        compiled.tick().unwrap();
    }
    // The retained certificate describes one edge; the opposite input must first deoptimize.
    compiled.use_block(trigger).unwrap();
    exact.use_block(trigger).unwrap();
    native_step(
        &mut native,
        Step::SetLever {
            position: trigger,
            powered: powered_before,
        },
    );
    assert!(compiled.future.symbolic_cursor.is_none());
    for _ in 0..8 {
        compiled.tick().unwrap();
        exact.tick().unwrap();
        native.tick_interpreted();
        assert_eq!(boundary(&compiled), boundary(&exact));
        check_boundary(&compiled, &native, &mut scratch, &positions, "opposite edge").unwrap();
    }
    compiled.reset(&mut scratch);
    for _ in 0..4 {
        scratch.tick_interpreted();
        native.tick_interpreted();
        assert_eq!(future(&scratch, &positions), future(&native, &positions));
    }
}

fn boundary(backend: &GraphBackend) -> MachineState {
    let mut state = MachineState::default();
    assert!(backend.capture_graph_boundary(&mut state));
    state
}
