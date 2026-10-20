//! Dense transport must preserve arbitrary strengths, sampling order and future work.
use super::*;
use crate::redpiler::CompilerOptions;

#[test]
fn dense_transport_preserves_unsettled_wire_strengths_and_request_provenance() {
    let mut world = empty_world();
    let source = BlockPos::new(42, 34, 42);
    world.set_block(source, Block::Lever { lever: Default::default() });
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
    let options = CompilerOptions::parse("-O5").unwrap();
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
    let mut reference = prepared.graph.clone();
    reference.plans.instant_transport = Default::default();
    let (reference_program, initial) = reference.lower().unwrap();
    let (candidate_program, _) = prepared.graph.lower().unwrap();
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
