//! Compiled hops must retain the general walk's complete ordered execution.
use super::*;
use crate::redpiler::passes::PassContext;
use crate::redpiler::passes::planning::PlanBuilder;
use mchprs_blocks::blocks::{RedstoneWire, RedstoneWireSide};

fn prepare_hops(world: &PlotWorld) -> CompileGraph {
    passes::rich::prepare(world, BOUNDS, vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph
}

fn compile_hops(graph: &mut CompileGraph) {
    passes::compile_wire_captures::CompileWireCaptures
        .run(
            graph,
            &PassContext::new(
                &TaskMonitor::default(),
                &Default::default(),
                &EntryState {
                    logical_tick: 0,
                    pending_ticks: vec![],
                },
            ),
        )
        .unwrap();
    passes::compile_wire_hops::CompileWireHops
        .run(
            graph,
            &PassContext::new(
                &TaskMonitor::default(),
                &Default::default(),
                &EntryState {
                    logical_tick: 0,
                    pending_ticks: vec![],
                },
            ),
        )
        .unwrap();
    passes::minimize_wire_captures::MinimizeWireCaptures
        .run(
            graph,
            &PassContext::new(
                &TaskMonitor::default(),
                &Default::default(),
                &EntryState {
                    logical_tick: 0,
                    pending_ticks: vec![],
                },
            ),
        )
        .unwrap();
    graph.validate().unwrap();
}

fn hop_backend(mut graph: CompileGraph, compiled: bool) -> GraphBackend {
    if compiled {
        compile_hops(&mut graph);
    }
    let (program, state) = graph.lower().unwrap();
    let mut backend = GraphBackend::new_unactivated(program, state, 0).unwrap();
    backend.set_test_recording(true);
    backend
}

pub(super) fn recording_hops_graph(world: &PlotWorld) -> GraphBackend {
    hop_backend(prepare_hops(world), true)
}

fn assert_walk_equivalent(general: &GraphBackend, compiled: &GraphBackend) {
    assert_eq!(compiled.state, general.state);
    let retained_trace = |backend: &GraphBackend| {
        backend
            .trace()
            .iter()
            .filter(|line| !(line.contains("wire discover") && line.contains("receiver=None properties=Passive")))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        retained_trace(compiled),
        retained_trace(general),
        "mutable captures, intermediate writes, headings and delivery layers"
    );
    assert_eq!(compiled.observed_operations, general.observed_operations);
    assert_eq!(compiled.scheduled_ticks(), general.scheduled_ticks());
    assert_eq!(compiled.counts, general.counts);
}

fn nested_wire_piston_world() -> PlotWorld {
    let mut world = empty_world();
    let bulb = BlockPos::new(6, 21, 5);
    let trigger = bulb.offset(BlockFace::East);
    let outer_wire = bulb.offset(BlockFace::Bottom);
    let north_piston = outer_wire.offset(BlockFace::North);
    let south_piston = outer_wire.offset(BlockFace::South);
    let foreign_piston = BlockPos::new(12, 20, 12);
    let foreign_power = foreign_piston.offset(BlockFace::West);

    world.set_block(trigger.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(trigger, lever(false));
    world.set_block(outer_wire.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(bulb, Block::from_name("copper_bulb").unwrap());
    world.set_block(
        outer_wire,
        Block::RedstoneWire {
            wire: RedstoneWire::new(
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                RedstoneWireSide::Side,
                0,
            ),
        },
    );
    world.set_block(foreign_power.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(foreign_power, lever(true));
    for (position, facing) in [
        (north_piston, BlockFace::North),
        (south_piston, BlockFace::South),
        (foreign_piston, BlockFace::East),
    ] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(
            position,
            Block::Piston {
                piston: mchprs_blocks::blocks::RedstonePiston {
                    facing: facing.into(),
                    sticky: false,
                    extended: false,
                },
            },
        );
    }
    world
}

fn actor_at(backend: &GraphBackend, position: BlockPos) -> PistonId {
    backend
        .program
        .pistons()
        .iter()
        .enumerate()
        .find_map(|(index, piston)| (piston.position == position).then_some(PistonId::new(index as u32)))
        .unwrap_or_else(|| panic!("expected a piston actor at {position:?}"))
}

#[test]
fn compiled_hops_preserve_all_analog_transitions_and_reuse_clean_scratch() {
    let graph = prepare_hops(&analog_dust_world());
    let mut general = hop_backend(graph.clone(), false);
    let mut compiled = hop_backend(graph, true);
    assert!(compiled.program.wires().iter().all(|wire| wire.execution.hop_root.is_some()));
    let input = general.positions[&LEVER_POS];
    let wires: Vec<_> = (0..3)
        .map(|distance| general.positions[&(NEXT_POS + BlockPos::new(distance, 0, 0))])
        .collect();
    for old in 0..=15 {
        for new in 0..=15 {
            for backend in [&mut general, &mut compiled] {
                backend.clear_test_traces();
                set_analog_dust_state(backend, input, &wires, old, new);
                backend.evaluate_seed(wires[0], None).unwrap();
            }
            assert_walk_equivalent(&general, &compiled);
            for walk in &compiled.wire_walks {
                assert!(walk.is_compiled());
                assert!(walk.sites.is_empty());
                assert!(walk.site_slots.is_empty(), "compiled execution must not insert into a hash table");
                assert!(walk.queues.iter().all(|queue| queue.is_empty()));
            }
        }
    }
    assert!(!compiled.wire_walks.is_empty(), "the compiled executor actually ran");
}

#[test]
fn compiled_capture_matches_general_for_every_site_and_saved_strength() {
    let mut backend = hop_backend(prepare_hops(&analog_dust_world()), true);
    for strength in 0..=15 {
        for state in &mut backend.state.components {
            match state {
                ComponentState::Wire(wire) => wire.power = strength,
                ComponentState::Electrical(state) => {
                    state.output_strength = strength;
                    state.powered = strength % 2 == 0;
                    state.repeater_locked = strength % 3 == 0;
                }
                _ => {}
            }
        }
        for index in 0..backend.program.sites().len() {
            let site = SiteId::new(index as u32);
            assert_eq!(backend.observe_site(site), backend.resolve_site(site, None).map(|site| site.observed()));
            assert_eq!(backend.observe_wire_site(site), backend.observe_site(site));
        }
    }
}

#[test]
fn compiled_hops_preserve_seeded_histories_on_branched_cyclic_dust() {
    let mut world = empty_world();
    for (x, z) in [(6, 5), (7, 5), (8, 5), (8, 6), (8, 7), (7, 7), (6, 7), (6, 6), (5, 6), (9, 6)] {
        let pos = BlockPos::new(x, 20, z);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(pos, Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(BlockPos::new(5, 19, 5), Block::Stone {});
    world.set_block(BlockPos::new(5, 20, 5), lever(false));
    let graph = prepare_hops(&world);
    let mut general = hop_backend(graph.clone(), false);
    let mut compiled = hop_backend(graph, true);
    let mut random = 0x4317_a29du32;
    for case in 0..128 {
        general.clear_test_traces();
        compiled.clear_test_traces();
        for state in &mut general.state.components {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let strength = (random >> 24) as u8 % 16;
            match state {
                ComponentState::Wire(wire) => wire.power = strength,
                ComponentState::Electrical(state) => {
                    state.powered = strength != 0;
                    state.output_strength = strength;
                }
                _ => {}
            }
        }
        compiled.state = general.state.clone();
        let root = general.program.wires()[case % general.program.wires().len()].node;
        general.evaluate_seed(root, None).unwrap();
        compiled.evaluate_seed(root, None).unwrap();
        assert_walk_equivalent(&general, &compiled);
    }
    assert!(compiled.counts.wire_walks > 0);
}

#[test]
fn compiled_capture_selects_live_contacts_and_properties_across_memory_poses() {
    let (world, bounds) = passes::simplify_instant_resets::tests::fixture();
    let graph = passes::rich::prepare(&world, bounds, vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph;
    let mut backend = hop_backend(graph, true);
    assert!(
        (0..backend.program.sites().len())
            .any(|index| matches!(backend.program.wire_capture(SiteId::new(index as u32)), Some(WireCapture::Contact(_))))
    );
    for pose in 0..4 {
        for state in &mut backend.state.pistons {
            state.extended = pose & 1 != 0;
            state.head_present = pose & 2 != 0;
        }
        for (state, spec) in backend.state.resources.iter_mut().zip(backend.program.resources()) {
            state.at = spec.contacts[pose % spec.contacts.len()];
            state.enabled = pose & 1 != 0;
        }
        for state in &mut backend.state.components {
            if let ComponentState::Electrical(state) = state {
                state.powered = pose & 1 != 0;
                state.repeater_locked = pose & 2 != 0;
                state.output_strength = if pose & 2 != 0 { 15 } else { 0 };
            }
        }
        for index in 0..backend.program.sites().len() {
            let site = SiteId::new(index as u32);
            assert_eq!(backend.observe_wire_site(site), backend.observe_site(site), "pose {pose}, site {index}");
        }
    }
}

#[test]
fn compiled_hops_preserve_nested_walks_and_outer_continuation() {
    let mut world = empty_world();
    let outer = LEVER_POS.offset(BlockFace::East);
    let bulb = outer.offset(BlockFace::South);
    let inner = bulb.offset(BlockFace::East);
    let source = inner.offset(BlockFace::North);
    let continuation = inner.offset(BlockFace::East);
    for position in [LEVER_POS, outer, inner, continuation] {
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
    }
    world.set_block(LEVER_POS, lever(false));
    world.set_block(source, lever(false));
    for position in [outer, inner, continuation] {
        world.set_block(position, Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(bulb, Block::from_name("copper_bulb").unwrap());
    world.set_block(bulb.offset(BlockFace::South), Block::RedstoneBlock);
    let graph = prepare_hops(&world);
    let mut general = hop_backend(graph.clone(), false);
    let mut compiled = hop_backend(graph, true);
    for backend in [&mut general, &mut compiled] {
        backend.set_node(backend.positions[&source], true, 15, None, false).unwrap();
        backend.use_block(LEVER_POS).unwrap();
    }
    assert_walk_equivalent(&general, &compiled);
    assert!(compiled.wire_walks.len() >= 2, "a real nested walk must own different scratch");
    assert!(
        compiled.trace().iter().any(|line| line.contains("-> 14")),
        "retain the intermediate wire write"
    );
    for _ in 0..3 {
        general.tick().unwrap();
        compiled.tick().unwrap();
        assert_walk_equivalent(&general, &compiled);
    }
}

#[test]
fn compiled_hops_preserve_nested_wire_updates_and_foreign_fifo_prefix() {
    let world = nested_wire_piston_world();
    let mut compiled = hop_backend(prepare_hops(&world), true);
    let mut general = hop_backend(prepare_hops(&world), false);

    let bulb = BlockPos::new(6, 21, 5);
    let trigger = bulb.offset(BlockFace::East);
    let outer_wire = bulb.offset(BlockFace::Bottom);
    let north = actor_at(&compiled, outer_wire.offset(BlockFace::North));
    let south = actor_at(&compiled, outer_wire.offset(BlockFace::South));
    let foreign_pos = BlockPos::new(12, 20, 12);
    let foreign = actor_at(&compiled, foreign_pos);
    let outer_node = compiled.positions[&outer_wire];
    let foreign_prefix = PistonRequest {
        piston: foreign,
        action: mchprs_world::PistonAction::Extend,
        source: None,
    };
    compiled.enqueue_piston(foreign_prefix);
    general.enqueue_piston(foreign_prefix);

    compiled.use_block(trigger).unwrap();
    general.use_block(trigger).unwrap();

    assert!(compiled.wire_state(outer_node).power > 0, "the bulb callback changes the outer wire");
    let north_powered = compiled.piston_powered(north);
    let south_powered = compiled.piston_powered(south);
    assert!(
        north_powered && south_powered,
        "the outer wire has strength {}, north piston powered={north_powered}, south piston powered={south_powered}",
        compiled.wire_state(outer_node).power
    );
    assert!(
        compiled.wire_walks.iter().any(|walk| walk.is_compiled()),
        "compiled wire hops actually run"
    );
    assert_eq!(
        compiled.future.piston_requests.front(),
        Some(&foreign_prefix),
        "the earlier foreign request stays at the FIFO head"
    );
    let callback_index = compiled
        .observed_operations
        .iter()
        .position(|operation| matches!(operation, GraphObservedOperation::Callback { target, .. } if *target == outer_wire))
        .expect("the bulb route callbacks into the outer wire");
    for piston_pos in [outer_wire.offset(BlockFace::North), outer_wire.offset(BlockFace::South)] {
        assert!(
            compiled.observed_operations.iter().enumerate().any(|(index, operation)| {
                index > callback_index && matches!(operation, GraphObservedOperation::Sample { actor, powered: true, .. } if *actor == piston_pos)
            }),
            "the {piston_pos:?} piston samples the updated wire after its callback"
        );
    }

    compiled.assert_same_execution_state(&general);
    assert_walk_equivalent(&general, &compiled);
    assert_eq!(compiled.callback_order, general.callback_order);
    assert_eq!(compiled.observed_operations, general.observed_operations);

    let native_trace_start = compiled.native_piston_trace().len();
    compiled.drain_piston_actions().unwrap();
    general.drain_piston_actions().unwrap();
    compiled.assert_same_execution_state(&general);
    assert_eq!(compiled.native_piston_trace(), general.native_piston_trace());
    assert_walk_equivalent(&general, &compiled);
    assert!(
        compiled.native_piston_trace()[native_trace_start..]
            .iter()
            .any(|entry| { matches!(entry.operation, crate::redstone::piston::trace::Operation::Sample { pos, .. } if pos == foreign_pos) }),
        "the preexisting foreign request reaches native acceptance before later requests"
    );

    let mut compiled_world = nested_wire_piston_world();
    let mut general_world = nested_wire_piston_world();
    compiled.flush(&mut compiled_world, false);
    general.flush(&mut general_world, false);
    for position in [bulb, outer_wire, outer_wire.offset(BlockFace::North), outer_wire.offset(BlockFace::South)] {
        assert_eq!(compiled_world.get_block(position), general_world.get_block(position));
    }
}
