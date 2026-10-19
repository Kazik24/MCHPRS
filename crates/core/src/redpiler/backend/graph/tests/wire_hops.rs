//! Compiled hops must retain the general walk's complete ordered execution.
use super::*;
use crate::redpiler::passes::PassContext;
use crate::redpiler::passes::planning::PlanBuilder;

fn prepare_hops(world: &PlotWorld) -> CompileGraph {
    passes::rich::prepare(world, BOUNDS, vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph
}

fn compile_hops(graph: &mut CompileGraph) {
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
}

fn hop_backend(mut graph: CompileGraph, compiled: bool) -> GraphBackend {
    if compiled {
        compile_hops(&mut graph);
    }
    let (program, state) = graph.lower().unwrap();
    let mut backend = GraphBackend::new(program, state, 0).unwrap();
    backend.set_test_recording(true);
    backend
}

pub(super) fn recording_hops_graph(world: &PlotWorld) -> GraphBackend {
    hop_backend(prepare_hops(world), true)
}

fn assert_walk_equivalent(general: &GraphBackend, compiled: &GraphBackend) {
    assert_eq!(compiled.state, general.state);
    assert_eq!(compiled.trace(), general.trace(), "intermediate writes, headings and delivery layers");
    assert_eq!(compiled.observed_operations, general.observed_operations);
    assert_eq!(compiled.scheduled_ticks(), general.scheduled_ticks());
    assert_eq!(compiled.counts, general.counts);
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
