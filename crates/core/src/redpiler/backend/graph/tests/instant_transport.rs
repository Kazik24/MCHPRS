//! Dense transport must preserve arbitrary strengths, sampling order and future work.
use super::*;
use crate::redpiler::CompilerOptions;

fn three_wire_graph() -> CompileGraph {
    three_wire_input_graph(false, false)
}

#[test]
fn transition_programs_replay_arbitrary_entry_profiles_and_preserve_boundary_order_and_future() {
    let mut graph = three_wire_input_graph(false, true);
    graph.plans.signal_channels = Default::default();
    graph.plans.instant_value_regions = Default::default();
    assert!(graph.plans.instant_transition_programs.nets[0].is_some());
    let net = graph.plans.instant_transport.nets[0].clone();
    let mut exact = graph.clone();
    exact.plans.instant_transition_programs = Default::default();
    let (program, initial) = graph.lower().unwrap();
    let (reference_program, _) = exact.lower().unwrap();
    let sources = [BlockPos::new(42, 34, 42), BlockPos::new(44, 34, 43)].map(|position| program.position_index()[&position]);
    let mut candidate = GraphBackend::new(program, initial.clone(), 0).unwrap();
    let mut reference = GraphBackend::new(reference_program, initial, 0).unwrap();
    for profile in 0..8 {
        for repeat in 0..2 {
            let before = candidate.execution_counts();
            for backend in [&mut candidate, &mut reference] {
                for (index, source) in sources.into_iter().enumerate() {
                    let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
                        unreachable!()
                    };
                    state.powered = profile & (1 << index) != 0;
                    state.output_strength = u8::from(state.powered) * 15;
                }
                for (index, wire) in net.wires.iter().enumerate() {
                    let mut value = backend.wire_state(wire.node);
                    value.power = ((profile * 5 + index * 7) % 16) as u8;
                    backend.set_wire(wire.node, value);
                }
                backend.set_test_recording(true);
                let drive = backend.transport_block_power(net.wires[0].node);
                backend.run_instant_transport(0, 0, drive).unwrap();
            }
            candidate.assert_same_execution_state(&reference);
            assert_eq!(
                candidate.observed_operations, reference.observed_operations,
                "profile={profile}, repeat={repeat}"
            );
            let after = candidate.execution_counts();
            if repeat == 1 {
                assert_eq!(after.instant_transition_hits, before.instant_transition_hits + 1);
                assert_eq!(after.instant_transport_wire_visits, before.instant_transport_wire_visits);
            }
        }
    }
    assert!(candidate.execution_counts().instant_transition_wire_visits_elided > 0);
    assert!(candidate.execution_counts().instant_transition_misses >= 8);
}

#[test]
fn transition_guard_ignores_wire_captured_only_for_neighbor_discovery() {
    let mut world = empty_world();
    for x in 42..=47 {
        world.set_block(BlockPos::new(x, 33, 42), Block::Stone {});
    }
    world.set_block(BlockPos::new(42, 34, 42), Block::Lever { lever: Default::default() });
    for x in 43..=47 {
        world.set_block(BlockPos::new(x, 34, 42), Block::RedstoneWire { wire: Default::default() });
    }
    world.set_block(
        BlockPos::new(48, 34, 42),
        Block::Piston {
            piston: mchprs_blocks::blocks::RedstonePiston {
                facing: mchprs_blocks::BlockFacing::East,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        BlockPos::new(49, 34, 42),
        Block::PistonHead {
            head: mchprs_blocks::blocks::RedstonePistonHead {
                facing: mchprs_blocks::BlockFacing::East,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(BlockPos::new(50, 34, 42), Block::RedstoneBlock);
    world.set_block(
        BlockPos::new(48, 35, 42),
        Block::Observer {
            observer: mchprs_blocks::blocks::RedstoneObserver {
                facing: mchprs_blocks::BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(BlockPos::new(48, 36, 42), Block::Stone {});
    let monitor = TaskMonitor::default();
    let options = CompilerOptions::parse("-O5").unwrap();
    let bounds = (BlockPos::new(40, 30, 40), BlockPos::new(53, 38, 49));
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let graph = prepared.graph;
    assert_eq!(graph.plans.instant_transport.nets.len(), 1);
    let net = graph.plans.instant_transport.nets[0].clone();
    assert!(graph.plans.instant_transition_programs.nets[0].is_some());
    assert!(net.wires.iter().all(|wire| wire.conditional_inputs == 0));

    let local_for_x = |x: i32, graph: &CompileGraph| {
        net.wires
            .iter()
            .position(|wire| graph.nodes[wire.node.index()].position == Some(BlockPos::new(x, 34, 42)))
            .unwrap() as u32
    };
    let root = local_for_x(43, &graph);
    let delivered = [44, 45].map(|x| local_for_x(x, &graph));
    let control = local_for_x(46, &graph);
    let far = local_for_x(47, &graph);
    let root_wire = &net.wires[root as usize];
    assert!(!root_wire.neighbors.contains(&net.wires[far as usize].site));
    assert!(
        delivered
            .iter()
            .all(|&wire| { root_wire.neighbors.iter().any(|&site| site == net.wires[wire as usize].site) })
    );
    let admission = graph
        .wires
        .iter()
        .find(|wire| wire.node == root_wire.node)
        .unwrap()
        .execution
        .callback_selection
        .unwrap();
    for &wire in &delivered {
        let slot = root_wire
            .neighbors
            .iter()
            .position(|&site| site == net.wires[wire as usize].site)
            .unwrap();
        assert_ne!(admission.known & (1 << slot), 0);
        assert_ne!(admission.admitted & (1 << slot), 0);
    }
    assert!(
        net.wires[delivered[1] as usize]
            .neighbors
            .iter()
            .any(|&site| site == net.wires[far as usize].site)
    );
    for &wire in &delivered {
        let row = &net.wires[wire as usize];
        let mut selected = row.inputs | row.conditional_inputs;
        while selected != 0 {
            let slot = selected.trailing_zeros() as usize;
            selected &= selected - 1;
            assert_ne!(row.neighbors[slot], net.wires[far as usize].site);
        }
    }
    let control_row = &net.wires[delivered[1] as usize];
    assert!((control_row.inputs | control_row.conditional_inputs) != 0);
    assert!(control_row.neighbors.iter().enumerate().any(|(slot, &site)| {
        site == net.wires[control as usize].site && ((control_row.inputs | control_row.conditional_inputs) & (1 << slot)) != 0
    }));

    let mut exact = graph.clone();
    exact.plans.instant_transition_programs = Default::default();
    let (program, initial) = graph.lower().unwrap();
    let (exact_program, _) = exact.lower().unwrap();
    let mut candidate = GraphBackend::new(program.clone(), initial.clone(), 0).unwrap();
    let mut reference = GraphBackend::new(exact_program, initial, 0).unwrap();
    let root_node = program.position_index()[&BlockPos::new(43, 34, 42)];

    for backend in [&mut candidate, &mut reference] {
        backend.set_test_recording(true);
        for wire in &net.wires {
            let mut state = backend.wire_state(wire.node);
            state.power = 0;
            backend.set_wire(wire.node, state);
        }
        let drive = backend.transport_block_power(root_node);
        assert_eq!(drive, 0, "the source remains off in this regression");
        backend.run_instant_transport(0, root, drive).unwrap();
    }
    candidate.assert_same_execution_state(&reference);
    assert_eq!(candidate.observed_operations, reference.observed_operations);
    assert_eq!(candidate.execution_counts().instant_transition_programs, 1);

    for backend in [&mut candidate, &mut reference] {
        let mut state = backend.wire_state(net.wires[far as usize].node);
        state.power = 15;
        backend.set_wire(net.wires[far as usize].node, state);
        let drive = backend.transport_block_power(root_node);
        backend.run_instant_transport(0, root, drive).unwrap();
    }
    candidate.assert_same_execution_state(&reference);
    assert_eq!(candidate.observed_operations, reference.observed_operations);
    assert_eq!(candidate.execution_counts().instant_transition_hits, 1);

    for backend in [&mut candidate, &mut reference] {
        let mut state = backend.wire_state(net.wires[control as usize].node);
        state.power = 15;
        backend.set_wire(net.wires[control as usize].node, state);
        let drive = backend.transport_block_power(root_node);
        backend.run_instant_transport(0, root, drive).unwrap();
    }
    candidate.assert_same_execution_state(&reference);
    assert_eq!(candidate.observed_operations, reference.observed_operations);
    assert_eq!(candidate.execution_counts().instant_transition_hits, 1);
    assert_eq!(candidate.execution_counts().instant_transition_misses, 2);
}

#[test]
fn value_regions_skip_settled_queries_but_recheck_each_walk_and_preserve_boundary_order() {
    let mut graph = three_wire_graph();
    graph.plans.instant_transition_programs = Default::default();
    graph.plans.signal_channels = Default::default();
    assert_eq!(graph.plans.instant_value_regions.stats.wires, 3);
    let net = graph.plans.instant_transport.nets[0].clone();
    let mut ordinary = graph.clone();
    ordinary.plans.instant_value_regions = Default::default();
    ordinary.plans.instant_transition_programs = Default::default();
    let (program, initial) = graph.lower().unwrap();
    let (reference_program, _) = ordinary.lower().unwrap();
    let source = program.position_index()[&BlockPos::new(42, 34, 42)];
    let mut candidate = GraphBackend::new(program, initial.clone(), 0).unwrap();
    let mut reference = GraphBackend::new(reference_program, initial, 0).unwrap();
    for powered in [false, true, false, true] {
        for unsettled in [false, true] {
            for root in 0..3 {
                let before = candidate.execution_counts();
                for backend in [&mut candidate, &mut reference] {
                    backend.set_test_recording(true);
                    let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
                        unreachable!()
                    };
                    state.powered = powered;
                    state.output_strength = 15 * u8::from(powered);
                    for (index, wire) in net.wires.iter().enumerate() {
                        let mut state = backend.wire_state(wire.node);
                        let x = backend.program.nodes()[wire.node.index()].position.unwrap().x;
                        state.power = if powered { 15 - (x - 43) as u8 } else { 0 };
                        if unsettled && index != root {
                            state.power = if state.power == 0 { 7 } else { 0 };
                        }
                        backend.set_wire(wire.node, state);
                    }
                    let drive = backend.transport_block_power(net.wires[root].node);
                    backend.run_instant_transport(0, root as u32, drive).unwrap();
                }
                candidate.assert_same_execution_state(&reference);
                assert_eq!(
                    candidate.observed_operations, reference.observed_operations,
                    "powered={powered}, unsettled={unsettled}, root={root}"
                );
                let after = candidate.execution_counts();
                assert!(
                    after.instant_value_region_solves + after.instant_value_region_cache_hits
                        > before.instant_value_region_solves + before.instant_value_region_cache_hits
                );
                if unsettled {
                    assert_eq!(after.instant_value_region_inert_visits, before.instant_value_region_inert_visits);
                } else {
                    assert_eq!(
                        after.instant_value_region_inert_visits - before.instant_value_region_inert_visits,
                        after.instant_transport_wire_visits - before.instant_transport_wire_visits
                    );
                    assert!(after.instant_value_region_inert_visits > before.instant_value_region_inert_visits);
                }
            }
        }
    }
}

#[test]
fn value_region_distance_rows_are_independently_validated() {
    let mut graph = three_wire_graph();
    let region = graph.plans.instant_value_regions.nets[0].as_mut().unwrap();
    assert!(!region.distances.is_empty());
    region.distances[0] ^= 1;
    assert!(graph.validate().is_err(), "a forged value row must not lower");
}

#[test]
fn value_region_cache_reuses_unchanged_state_and_invalidates_power_and_shape_writes() {
    let mut graph = three_wire_graph();
    graph.plans.instant_transition_programs = Default::default();
    graph.plans.signal_channels = Default::default();
    let net = graph.plans.instant_transport.nets[0].clone();
    let mut ordinary = graph.clone();
    ordinary.plans.instant_value_regions = Default::default();
    ordinary.plans.instant_transition_programs = Default::default();
    let (program, initial) = graph.lower().unwrap();
    let (reference_program, _) = ordinary.lower().unwrap();
    let mut candidate = GraphBackend::new(program, initial.clone(), 0).unwrap();
    let mut reference = GraphBackend::new(reference_program, initial, 0).unwrap();
    for phase in 0..5 {
        let before = candidate.execution_counts();
        for backend in [&mut candidate, &mut reference] {
            backend.set_test_recording(true);
            if phase == 2 || phase == 3 {
                let node = net.wires[1].node;
                let mut wire = backend.wire_state(node);
                if phase == 2 {
                    wire.north = if wire.north == mchprs_blocks::blocks::RedstoneWireSide::None {
                        mchprs_blocks::blocks::RedstoneWireSide::Side
                    } else {
                        mchprs_blocks::blocks::RedstoneWireSide::None
                    };
                } else {
                    wire.power = 7;
                }
                backend.set_wire(node, wire);
            }
            backend.run_instant_transport(0, 0, 0).unwrap();
        }
        candidate.assert_same_execution_state(&reference);
        assert_eq!(candidate.observed_operations, reference.observed_operations, "phase={phase}");
        let after = candidate.execution_counts();
        if phase == 1 {
            assert_eq!(after.instant_value_region_solves, before.instant_value_region_solves);
            assert!(after.instant_value_region_cache_hits > before.instant_value_region_cache_hits);
        } else {
            assert!(after.instant_value_region_solves > before.instant_value_region_solves, "phase={phase}");
        }
        assert!(after.instant_transport_compact_captures > before.instant_transport_compact_captures);
    }
}

#[test]
fn compact_wire_captures_require_all_inbound_slot_proofs_and_preserve_full_capture_fallback() {
    let mut graph = three_wire_graph();
    graph.plans.instant_transition_programs = Default::default();
    graph.plans.signal_channels = Default::default();
    assert!(
        graph.plans.instant_value_regions.nets[0]
            .as_ref()
            .unwrap()
            .compact_wire_captures
            .iter()
            .all(|&value| value)
    );
    for wire in &mut graph.wires {
        wire.execution.callback_selection = None;
    }
    graph.plans.instant_value_regions = infer_instant_value_regions(
        &graph,
        &graph.plans.instant_transport,
        &graph.plans.instant_drives,
        &TaskMonitor::default(),
    )
    .unwrap();
    assert!(
        graph.plans.instant_value_regions.nets[0]
            .as_ref()
            .unwrap()
            .compact_wire_captures
            .iter()
            .all(|&value| !value)
    );
    graph.plans.instant_value_regions.nets[0].as_mut().unwrap().compact_wire_captures[0] = true;
    assert!(
        graph.validate().is_err(),
        "unknown inbound admission cannot be forged into a scalar capture"
    );
    graph.plans.instant_value_regions.nets[0].as_mut().unwrap().compact_wire_captures[0] = false;
    let mut ordinary = graph.clone();
    ordinary.plans.instant_value_regions = Default::default();
    ordinary.plans.instant_transition_programs = Default::default();
    let net = graph.plans.instant_transport.nets[0].clone();
    let (program, initial) = graph.lower().unwrap();
    let (reference_program, _) = ordinary.lower().unwrap();
    let mut candidate = GraphBackend::new(program, initial.clone(), 0).unwrap();
    let mut reference = GraphBackend::new(reference_program, initial, 0).unwrap();
    for backend in [&mut candidate, &mut reference] {
        backend.set_test_recording(true);
        for (index, wire) in net.wires.iter().enumerate() {
            let mut state = backend.wire_state(wire.node);
            state.power = [7, 15, 0][index];
            backend.set_wire(wire.node, state);
        }
        backend.run_instant_transport(0, 0, 0).unwrap();
    }
    candidate.assert_same_execution_state(&reference);
    assert_eq!(candidate.observed_operations, reference.observed_operations);
    assert_eq!(candidate.execution_counts().instant_transport_compact_captures, 0);
}

fn three_wire_input_graph(stable_inputs: bool, second_input: bool) -> CompileGraph {
    let mut world = empty_world();
    let source = BlockPos::new(42, 34, 42);
    world.set_block(source, Block::Lever { lever: Default::default() });
    if second_input {
        world.set_block(BlockPos::new(44, 33, 43), Block::Stone {});
        world.set_block(BlockPos::new(44, 34, 43), Block::Lever { lever: Default::default() });
    }
    for x in 42..=45 {
        world.set_block(BlockPos::new(x, 33, 42), Block::Stone {});
        if x > 42 {
            world.set_block(BlockPos::new(x, 34, 42), Block::RedstoneWire { wire: Default::default() });
        }
    }
    world.set_block(
        BlockPos::new(46, 34, 42),
        Block::Piston {
            piston: mchprs_blocks::blocks::RedstonePiston {
                facing: mchprs_blocks::BlockFacing::East,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        BlockPos::new(47, 34, 42),
        Block::PistonHead {
            head: mchprs_blocks::blocks::RedstonePistonHead {
                facing: mchprs_blocks::BlockFacing::East,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(BlockPos::new(48, 34, 42), Block::RedstoneBlock);
    world.set_block(
        BlockPos::new(46, 35, 42),
        Block::Observer {
            observer: mchprs_blocks::blocks::RedstoneObserver {
                facing: mchprs_blocks::BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(BlockPos::new(46, 36, 42), Block::Stone {});
    let monitor = TaskMonitor::default();
    let mut options = CompilerOptions::parse("-O5").unwrap();
    options.stable_instant_inputs = stable_inputs;
    let bounds = (BlockPos::new(40, 30, 40), BlockPos::new(51, 38, 49));
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    assert_eq!(prepared.graph.plans.instant_transport.nets.len(), 1);
    assert_eq!(prepared.graph.plans.instant_transport.nets[0].wires.len(), 3);
    let transport = &prepared.graph.plans.instant_transport.nets[0];
    let local_sites: rustc_hash::FxHashMap<_, _> = transport
        .sites
        .iter()
        .copied()
        .enumerate()
        .map(|(local, site)| (site, local as u32))
        .collect();
    let mut shared_retained_occurrences = 0;
    let mut retained_occurrences_by_site = rustc_hash::FxHashMap::<SiteId, usize>::default();
    for row in &transport.wires {
        let CircuitNodeKind::Wire(index) = prepared.graph.nodes[row.node.index()].kind else {
            unreachable!()
        };
        for (slot, &site) in prepared.graph.wires[index].canonical_neighbors.iter().enumerate() {
            if let Some(&local) = local_sites.get(&site) {
                assert_eq!(
                    row.neighbors[slot], local,
                    "shared first-capture/layer site {site:?} must survive in every source row"
                );
                shared_retained_occurrences += 1;
                *retained_occurrences_by_site.entry(site).or_default() += 1;
            }
        }
    }
    assert!(shared_retained_occurrences > transport.wires.len());
    assert!(retained_occurrences_by_site.values().any(|&count| count > 1));
    prepared.graph
}

#[test]
fn stable_logic_adder11_preserves_all_pinned_output_windows() {
    let circuit = load_adder("adder_11bits", ORIGIN, None);
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let options = CompilerOptions::parse("-O5 --stable-instant-inputs").unwrap();
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    assert!(prepared.graph.plans.stable_instant_logic.stats.admitted > 0);
    let (program, initial) = prepared.graph.lower().unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_11bits.json")).unwrap()).unwrap();
    let mut checked = 0;
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
    {
        let mut native = circuit.build();
        let mut projected = circuit.build();
        let mut candidate = GraphBackend::new(program.clone(), initial.clone(), 0).unwrap();
        let mut steps = case_steps(&circuit, case, false, true);
        let response = steps.len();
        steps.extend(std::iter::repeat_n(Step::Tick, 16));
        for (index, step) in steps.into_iter().enumerate() {
            let changed = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut candidate, step, changed).unwrap();
            if index >= response {
                project(&candidate, &mut projected);
                let expected = expectation(&circuit, case);
                assert_math_window(&circuit, &native, expected.0, expected.1, index - response + 1);
                assert_math_window(&circuit, &projected, expected.0, expected.1, index - response + 1);
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 40);
}

#[test]
fn stable_logic_evaluates_independent_inputs_as_one_combinational_program() {
    let graph = three_wire_input_graph(true, true);
    assert_eq!(graph.plans.stable_instant_logic.stats.admitted, 1);
    assert_eq!(graph.plans.stable_instant_logic.stats.wires, 3);
    assert!(graph.plans.stable_instant_logic.stats.inputs >= 2);
    let net = graph.plans.instant_transport.nets[0].clone();
    let (program, initial) = graph.lower().unwrap();
    let sources = [BlockPos::new(42, 34, 42), BlockPos::new(44, 34, 43)].map(|position| program.position_index()[&position]);
    for a in [false, true] {
        for b in [false, true] {
            let mut backend = GraphBackend::new(program.clone(), initial.clone(), 0).unwrap();
            for (source, powered) in sources.into_iter().zip([a, b]) {
                let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
                    unreachable!()
                };
                state.powered = powered;
                state.output_strength = 7;
            }
            let root = if a {
                0
            } else if b {
                1
            } else {
                0
            };
            backend.evaluate_seed(net.wires[root].node, None).unwrap();
            let strengths = [
                (u8::from(a) * 15).max((u8::from(b) * 15).saturating_sub(1)),
                (u8::from(b) * 15).max((u8::from(a) * 15).saturating_sub(1)),
                (u8::from(a) * 15).saturating_sub(2).max((u8::from(b) * 15).saturating_sub(1)),
            ];
            let actual: Vec<_> = net.wires.iter().map(|wire| backend.wire_state(wire.node).power).collect();
            assert_eq!(actual, strengths, "inputs {a} {b}");
            assert_eq!(backend.execution_counts().stable_logic_invocations, 1);
            assert_eq!(backend.execution_counts().wire_walks, 0);
            assert_eq!(backend.execution_counts().instant_transport_wire_visits, 0);
            let before = backend.execution_counts().stable_logic_wire_values;
            backend.evaluate_seed(net.wires[root].node, None).unwrap();
            assert_eq!(
                backend.execution_counts().stable_logic_wire_values - before,
                1,
                "unchanged input must evaluate only its entry expression"
            );
        }
    }
    let ordinary = three_wire_input_graph(false, true);
    assert!(
        ordinary.plans.stable_instant_logic.nets.is_empty(),
        "the input-hold contract must be explicit"
    );
}

#[test]
fn stable_logic_cached_updates_match_full_solves_and_invalidate_on_wire_writes() {
    let graph = three_wire_input_graph(true, true);
    let net = graph.plans.instant_transport.nets[0].clone();
    let (program, initial) = graph.lower().unwrap();
    let sources = [BlockPos::new(42, 34, 42), BlockPos::new(44, 34, 43)].map(|position| program.position_index()[&position]);
    let mut cached = GraphBackend::new(program.clone(), initial.clone(), 0).unwrap();
    let mut full = GraphBackend::new(program, initial, 0).unwrap();
    cached.set_test_recording(true);
    full.set_test_recording(true);
    for step in 0..128usize {
        for backend in [&mut cached, &mut full] {
            for (bit, source) in sources.iter().enumerate() {
                let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
                    unreachable!()
                };
                state.powered = (step >> bit) & 1 != 0;
            }
            if step % 7 == 0 {
                let wire = net.wires[(step / 7) % net.wires.len()].node;
                let mut value = backend.wire_state(wire);
                value.power = (step % 16) as u8;
                backend.set_wire(wire, value);
            }
        }
        full.stable_logic_cache.clear();
        let node = net.wires[(step / 4) % net.wires.len()].node;
        cached.evaluate_seed(node, None).unwrap();
        full.evaluate_seed(node, None).unwrap();
        cached.assert_same_execution_state(&full);
        assert_eq!(cached.observed_operations, full.observed_operations, "step {step}");
    }
    let mut stale = cached.observe_site(cached.component_sites[net.wires[0].node.index()].unwrap()).unwrap();
    let ObservedProperties::Wire(wire) = &mut stale.properties else {
        unreachable!()
    };
    wire.power = (wire.power + 1) % 16;
    assert!(!cached.try_stable_instant_logic(net.wires[0].node, Some(&stale)).unwrap());
    assert!(!cached.stable_logic_cache[0].valid);
}

#[test]
fn stable_logic_changes_only_expressions_reachable_from_changed_inputs() {
    let mut world = empty_world();
    for x in 42..=75 {
        world.set_block(BlockPos::new(x, 33, 42), Block::Stone {});
        world.set_block(
            BlockPos::new(x, 34, 42),
            if x == 42 || x == 75 {
                Block::Lever { lever: Default::default() }
            } else {
                Block::RedstoneWire { wire: Default::default() }
            },
        );
    }
    world.set_block(
        BlockPos::new(74, 34, 43),
        Block::Piston {
            piston: mchprs_blocks::blocks::RedstonePiston {
                facing: mchprs_blocks::BlockFacing::South,
                sticky: true,
                extended: true,
            },
        },
    );
    world.set_block(
        BlockPos::new(74, 34, 44),
        Block::PistonHead {
            head: mchprs_blocks::blocks::RedstonePistonHead {
                facing: mchprs_blocks::BlockFacing::South,
                sticky: true,
                short: false,
            },
        },
    );
    world.set_block(BlockPos::new(74, 34, 45), Block::RedstoneBlock);
    world.set_block(
        BlockPos::new(74, 35, 43),
        Block::Observer {
            observer: mchprs_blocks::blocks::RedstoneObserver {
                facing: mchprs_blocks::BlockFacing::Down,
                powered: false,
            },
        },
    );
    world.set_block(BlockPos::new(74, 36, 43), Block::Stone {});
    let monitor = TaskMonitor::default();
    let options = CompilerOptions::parse("-O5 --stable-instant-inputs").unwrap();
    let mut prepared = passes::rich::prepare(
        &world,
        (BlockPos::new(40, 30, 40), BlockPos::new(78, 38, 46)),
        vec![],
        &monitor,
        AnalysisLimits::default(),
    )
    .unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    assert_eq!(
        prepared.graph.plans.stable_instant_logic.stats.admitted, 1,
        "{:?}",
        prepared.graph.plans.instant_transport.stats
    );
    assert_eq!(prepared.graph.plans.stable_instant_logic.stats.wires, 32);
    let (program, initial) = prepared.graph.lower().unwrap();
    let source = program.position_index()[&BlockPos::new(42, 34, 42)];
    let root = program.position_index()[&BlockPos::new(43, 34, 42)];
    let mut backend = GraphBackend::new(program, initial, 0).unwrap();
    for powered in [true, false, true, false] {
        let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
            unreachable!()
        };
        state.powered = powered;
        let before = backend.execution_counts().stable_logic_wire_values;
        backend.evaluate_seed(root, None).unwrap();
        let evaluated = backend.execution_counts().stable_logic_wire_values - before;
        assert_eq!(evaluated, if before == 0 { 32 } else { 15 });
        for x in 43..=74 {
            let node = backend.program.position_index()[&BlockPos::new(x, 34, 42)];
            assert_eq!(
                backend.wire_state(node).power,
                if powered { 15u8.saturating_sub((x - 43) as u8) } else { 0 }
            );
        }
    }
}

#[test]
fn signal_channels_execute_exact_ordered_transitions_and_reject_stale_entries() {
    let graph = three_wire_graph();
    assert_eq!(graph.plans.signal_channels.stats.nets, 1);
    let channel = graph.plans.signal_channels.channels[0].as_ref().unwrap().clone();
    let net = graph.plans.instant_transport.nets[0].clone();
    let mut ordinary = graph.clone();
    ordinary.clear_execution_plans();
    let (reference_program, initial) = ordinary.lower().unwrap();
    let (candidate_program, _) = graph.lower().unwrap();
    let source = candidate_program.position_index()[&BlockPos::new(42, 34, 42)];
    let mut executed_ops = 0;
    for old in 0..2 {
        for new in 0..2 {
            for root in 0..net.wires.len() {
                for pose in 0..8 {
                    let mut reference = GraphBackend::new(reference_program.clone(), initial.clone(), 0).unwrap();
                    let mut candidate = GraphBackend::new(candidate_program.clone(), initial.clone(), 0).unwrap();
                    for backend in [&mut reference, &mut candidate] {
                        backend.set_test_recording(true);
                        let ComponentState::Electrical(state) = &mut backend.state.components[source.index()] else {
                            unreachable!()
                        };
                        state.powered = new != 0;
                        state.output_strength = 7;
                        backend.state.pistons[0].extended = pose & 1 != 0;
                        backend.state.pistons[0].head_present = pose & 2 != 0;
                        for resource in backend.state.resources.iter_mut() {
                            resource.enabled = pose & 4 != 0;
                        }
                        for (wire, &power) in net.wires.iter().zip(&channel.profiles[old]) {
                            let mut state = backend.wire_state(wire.node);
                            state.power = power;
                            backend.set_wire(wire.node, state);
                        }
                    }
                    reference.evaluate_seed(net.wires[root].node, None).unwrap();
                    candidate.evaluate_seed(net.wires[root].node, None).unwrap();
                    candidate.assert_same_execution_state(&reference);
                    assert_eq!(
                        candidate.observed_operations, reference.observed_operations,
                        "old={old} new={new} root={root} pose={pose}"
                    );
                    assert_eq!(candidate.execution_counts().signal_channel_invocations, 1);
                    assert_eq!(candidate.execution_counts().instant_transport_wire_visits, 0);
                    assert_eq!(candidate.execution_counts().wire_walks, 0);
                    executed_ops += candidate.execution_counts().signal_channel_operations;
                }
            }
        }
    }
    assert!(executed_ops > 0);

    let mut reference = GraphBackend::new(reference_program, initial.clone(), 0).unwrap();
    let mut candidate = GraphBackend::new(candidate_program, initial, 0).unwrap();
    let node = net.wires[0].node;
    let site = candidate.component_sites[node.index()].unwrap();
    let mut captured = candidate.observe_wire_site(site).unwrap();
    let ObservedProperties::Wire(ref mut wire) = captured.properties else {
        unreachable!()
    };
    wire.power = 3;
    for backend in [&mut reference, &mut candidate] {
        backend.set_test_recording(true);
        backend.evaluate_seed(node, Some(&captured)).unwrap();
    }
    candidate.assert_same_execution_state(&reference);
    assert_eq!(candidate.observed_operations, reference.observed_operations);
    assert_eq!(candidate.execution_counts().signal_channel_invocations, 0);
    assert_eq!(candidate.execution_counts().signal_channel_fallbacks, 1);
}

#[test]
fn signal_channel_plans_reject_forgery_and_cancel_before_publication() {
    let mut graph = three_wire_graph();
    let monitor = TaskMonitor::default();
    monitor.cancel();
    assert!(infer_signal_channels(&graph, &monitor).is_err());
    let channel = graph.plans.signal_channels.channels[0].as_mut().unwrap();
    let program = channel.transitions.iter_mut().flatten().next().unwrap();
    let power = program
        .ops
        .iter_mut()
        .find_map(|op| match op {
            SignalOp::SetPower { power, .. } => Some(power),
            _ => None,
        })
        .unwrap();
    *power ^= 1;
    assert!(graph.validate().is_err());
}

#[test]
fn typed_instant_drives_match_general_reads_across_live_adder_states() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let options = CompilerOptions::parse("-O5").unwrap();
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let mut ordinary = prepared.graph.clone();
    ordinary.clear_execution_plans();
    let (program, state) = ordinary.lower().unwrap();
    let mut reference = GraphBackend::new(program, state, 0).unwrap();
    let (program, state) = prepared.graph.lower().unwrap();
    let mut candidate = GraphBackend::new(program, state, 0).unwrap();
    let wires: Vec<_> = candidate.program.wires().iter().map(|plan| plan.node).collect();
    let mut compared = 0;
    for strength in 0..16u8 {
        for phase in 0..16usize {
            for (index, component) in candidate.state.components.iter_mut().enumerate() {
                match component {
                    ComponentState::Wire(wire) => wire.power = (strength + (index % 16) as u8) % 16,
                    ComponentState::Electrical(state) => {
                        // Imported snapshots can disagree on the Boolean and scalar source state.
                        state.powered = (index + phase) & 1 != 0;
                        state.output_strength = (strength + ((index + phase) % 16) as u8) % 16;
                    }
                    ComponentState::Specialized => {}
                }
            }
            for (index, piston) in candidate.state.pistons.iter_mut().enumerate() {
                piston.extended = (index + phase) & 1 != 0;
                piston.head_present = (index + phase) & 2 != 0;
            }
            for (index, resource) in candidate.state.resources.iter_mut().enumerate() {
                let contacts = &candidate.program.resources()[index].contacts;
                resource.at = contacts[(index + phase) % contacts.len()];
                resource.enabled = (index + phase) & 2 != 0;
            }
            reference.state = candidate.state.clone();
            for &wire in &wires {
                if let Some(actual) = candidate.instant_drive_power(wire) {
                    assert_eq!(
                        actual,
                        reference.transport_block_power(wire),
                        "wire={wire:?}, strength={strength}, phase={phase}"
                    );
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 0, "the test must exercise compiled source expressions");
}

#[test]
fn dense_transport_preserves_unsettled_wire_strengths_and_request_provenance() {
    let graph = three_wire_graph();
    let mut reference = graph.clone();
    reference.plans.instant_transport = Default::default();
    reference.plans.instant_drives = Default::default();
    reference.plans.instant_value_regions = Default::default();
    reference.plans.instant_transition_programs = Default::default();
    reference.plans.signal_channels = Default::default();
    let (reference_program, initial) = reference.lower().unwrap();
    let (candidate_program, _) = graph.lower().unwrap();
    let wires: Vec<_> = (43..=45).map(|x| reference_program.position_index()[&BlockPos::new(x, 34, 42)]).collect();
    let mut invocations = 0;
    for packed in 0..4096u32 {
        for root in 0..3 {
            let mut reference = GraphBackend::new(reference_program.clone(), initial.clone(), 0).unwrap();
            let mut candidate = GraphBackend::new(candidate_program.clone(), initial.clone(), 0).unwrap();
            for backend in [&mut reference, &mut candidate] {
                backend.set_test_recording(true);
                for (index, &node) in wires.iter().enumerate() {
                    let mut state = backend.wire_state(node);
                    state.power = ((packed >> (4 * index)) & 15) as u8;
                    backend.set_wire(node, state);
                }
                backend.evaluate_seed(wires[root], None).unwrap();
            }
            candidate.assert_same_execution_state(&reference);
            assert_eq!(
                candidate.observed_operations, reference.observed_operations,
                "powers={packed:x}, root={root}"
            );
            invocations += candidate.execution_counts().instant_transport_invocations;
        }
    }
    assert!(invocations > 0, "the dense executor must actually run");
}

#[test]
fn compiled_signal_program_preserves_all_strengths_live_pistons_and_ordered_future() {
    let graph = three_wire_graph();
    let net = graph.plans.instant_transport.nets[0].clone();
    let mut compiler = crate::redpiler::backend::graph::transport::TransportReference::new(&graph).unwrap();
    let mut ordinary = graph.clone();
    ordinary.plans.instant_transport = Default::default();
    ordinary.plans.instant_drives = Default::default();
    ordinary.plans.instant_value_regions = Default::default();
    ordinary.plans.instant_transition_programs = Default::default();
    ordinary.plans.signal_channels = Default::default();
    let (program, initial) = ordinary.lower().unwrap();
    let monitor = TaskMonitor::default();
    let mut calls = 0;
    for packed in 0..4096u32 {
        let old: Vec<_> = (0..net.wires.len()).map(|wire| ((packed >> (4 * wire)) & 15) as u8).collect();
        for root in 0..net.wires.len() {
            let transcript = compiler.compile(&net, root as u32, &old, &vec![0; net.wires.len()], &monitor).unwrap();
            let mut reference = GraphBackend::new(program.clone(), initial.clone(), 0).unwrap();
            let mut candidate = GraphBackend::new(program.clone(), initial.clone(), 0).unwrap();
            for backend in [&mut reference, &mut candidate] {
                backend.set_test_recording(true);
                // The transcript must not freeze the reference generator's receiver pose.
                backend.state.pistons[0].extended = packed & 1 == 0;
                for (wire, &power) in net.wires.iter().zip(&old) {
                    let mut value = backend.wire_state(wire.node);
                    value.power = power;
                    backend.set_wire(wire.node, value);
                }
            }
            reference.evaluate_seed(net.wires[root].node, None).unwrap();
            candidate.run_signal_transcript(&net, &transcript).unwrap();
            candidate.assert_same_execution_state(&reference);
            assert_eq!(
                candidate.observed_operations, reference.observed_operations,
                "profile={packed:x}, root={root}"
            );
            for (wire, &power) in net.wires.iter().zip(&transcript.final_powers) {
                assert_eq!(candidate.wire_state(wire.node).power, power);
            }
            assert_eq!(candidate.execution_counts().wire_walks, 0);
            calls += transcript.ops.iter().filter(|op| matches!(op, SignalOp::Call { .. })).count();
        }
    }
    assert!(calls > 0, "specialized programs must exercise live boundary effects");
}

#[test]
fn dense_transport_preserves_adder_fifo_during_reset_and_native_handoff() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let mut native = circuit.build();
    let mut projected = circuit.build();
    let monitor = TaskMonitor::default();
    let options = CompilerOptions::parse("-O5").unwrap();
    let mut prepared = passes::rich::prepare(&native, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let mut reference = prepared.graph.clone();
    reference.plans.instant_transport = Default::default();
    reference.plans.instant_drives = Default::default();
    reference.plans.instant_value_regions = Default::default();
    reference.plans.instant_transition_programs = Default::default();
    reference.plans.signal_channels = Default::default();
    let (program, state) = reference.lower().unwrap();
    let mut reference = GraphBackend::new(program, state, 0).unwrap();
    let mut candidate = GraphBackend::from_lowered(prepared.lower(&monitor).unwrap(), &monitor).unwrap();
    reference.set_test_recording(true);
    candidate.set_test_recording(true);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_1bit.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "prepared-0-1-1")
        .unwrap();
    let mut steps = case_steps(&circuit, case, false, true);
    steps.extend(std::iter::repeat_n(Step::Tick, 24));
    let positions = circuit.positions();
    for (index, step) in steps.into_iter().enumerate() {
        let changed = match step {
            Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever { lever } if lever.powered != powered),
            Step::Tick => false,
        };
        native_step(&mut native, step);
        graph_step(&mut reference, step, changed).unwrap();
        graph_step(&mut candidate, step, changed).unwrap();
        candidate.assert_same_execution_state(&reference);
        assert_eq!(candidate.observed_operations, reference.observed_operations, "step {index}");
        check_boundary(&candidate, &native, &mut projected, &positions, &format!("dense transport step {index}")).unwrap();
    }
    assert!(candidate.execution_counts().instant_transport_invocations > 0);
    let mut resumed = circuit.build();
    candidate.reset(&mut resumed);
    for _ in 0..16 {
        native.tick_interpreted();
        resumed.tick_interpreted();
        assert_eq!(first_difference(&future(&native, &positions), &future(&resumed, &positions)), None);
    }
}
