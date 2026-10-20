//! Stateful kernels must execute shared owners without consuming the surrounding FIFO.
use super::*;
use crate::redpiler::{CompilerOptions, passes::planning::PlanBuilder};

fn prepare(circuit: &Adder) -> (LoweredCircuit, LoweredCircuit) {
    prepare_mode(circuit, false)
}

fn prepare_mode(circuit: &Adder, cpu: bool) -> (LoweredCircuit, LoweredCircuit) {
    let native = circuit.build();
    let monitor = TaskMonitor::default();
    let options = CompilerOptions {
        optimization_level: Some(5),
        stable_instant_inputs: true,
        ..Default::default()
    };
    let mut graph = passes::rich::prepare(&native, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let mut pipeline = passes::default_graph_pipeline(&options);
    pipeline
        .entries
        .retain(|entry| !matches!(&entry.pass, passes::Pass::Build(builder) if matches!(builder.name(), "experimental-compile-instant-events" | "experimental-compile-bud-memory-reads" | "optimize-instant-kernels-cpu")));
    passes::run_graph_pipeline(&mut graph.graph, &options, &graph.entry, &pipeline, &monitor).unwrap();
    let baseline = graph.graph.clone().lower().unwrap();
    passes::compile_bud_memory_reads::CompileBudMemoryReads
        .run(&mut graph.graph, &passes::PassContext::new(&monitor, &options, &graph.entry))
        .unwrap();
    passes::compile_instant_events::CompileInstantEvents
        .run(&mut graph.graph, &passes::PassContext::new(&monitor, &options, &graph.entry))
        .unwrap();
    assert_reduced_predicates(&graph.graph);
    if cpu {
        passes::optimize_instant_kernels_cpu::OptimizeInstantKernelsCpu
            .run(&mut graph.graph, &passes::PassContext::new(&monitor, &options, &graph.entry))
            .unwrap();
        let packed = graph.graph.plans.cpu_kernels.clone();
        assert!(!packed.entries.is_empty());
        graph.graph.plans.cpu_kernels.require_true[0] ^= 1;
        assert!(graph.graph.validate().is_err(), "forged CPU predicate must not lower");
        graph.graph.plans.cpu_kernels = packed;
    }
    graph.graph.validate().unwrap();
    let exported = crate::redpiler::analysis::export_json::from_graph(&graph.report, &graph.graph)
        .unwrap()
        .to_bytes()
        .unwrap();
    if cpu {
        let directory = root().join("target/cpu-kernels");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("packed-test.graph.json"), exported).unwrap();
    }
    let entry = graph.graph.plans.instant_events[0].clone();
    graph.graph.plans.instant_events[0].actors = Box::new([]);
    assert!(graph.graph.validate().is_err(), "forged event ownership must not lower");
    graph.graph.plans.instant_events[0] = entry;
    let candidate = graph.graph.lower().unwrap();
    let lowered = |(program, state)| LoweredCircuit {
        program,
        state,
        logical_tick: 0,
        positions: Default::default(),
        imported_ticks: vec![],
    };
    (lowered(baseline), lowered(candidate))
}

fn assert_reduced_predicates(graph: &CompileGraph) {
    let entries: std::collections::HashMap<_, _> = graph
        .plans
        .instant_events
        .iter()
        .flat_map(|kernel| match &kernel.program {
            KernelProgram::Events(entries) => entries.iter(),
            _ => unreachable!(),
        })
        .map(|entry| (entry.actor, entry))
        .collect();
    let mut random = 0x19a4_110bu32;
    for cell in &graph.plans.instant.boundaries.as_ref().unwrap().cells {
        for use_ in &cell.uses {
            let entry = entries[&use_.actor];
            let reduced = CellContract {
                uses: Box::new([]),
                resources: Box::new([]),
                pins: entry.pins.clone(),
                stationary_entry: entry.valid,
            };
            let mut states = std::collections::HashMap::new();
            for pin in &cell.pins {
                if let CellPin::Read(CellRead::State(test)) = pin {
                    states.insert(*test, false);
                }
            }
            for _ in 0..64 {
                for value in states.values_mut() {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    *value = random >> 31 != 0;
                }
                states.insert(StateTest::PistonExtended(use_.actor), true);
                states.insert(
                    StateTest::PistonAvailability {
                        piston: use_.actor,
                        is: PistonAvailability::Ready,
                    },
                    true,
                );
                let read = |read| match read {
                    CellRead::State(test) => states[&test],
                    CellRead::Activation(_) | CellRead::Contribution(_) => false,
                };
                let mut full = Vec::new();
                let mut compact = Vec::new();
                cell.evaluate(&mut full, read).unwrap();
                reduced.evaluate(&mut compact, read).unwrap();
                assert_eq!(
                    full[use_.valid.index()],
                    compact[entry.valid.index()],
                    "reduced acceptance for actor {:?}",
                    use_.actor
                );
            }
        }
    }
}

#[test]
fn bud_memory_read_certificates_reject_extra_inputs_and_variable_guards() {
    let circuit = boundary_contract_tests::load_mechanism("counter_basic");
    let world = circuit.build();
    let monitor = TaskMonitor::default();
    let options = CompilerOptions {
        optimization_level: Some(5),
        stable_instant_inputs: true,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(&world, circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let mut pipeline = passes::default_graph_pipeline(&options);
    pipeline.entries.retain(|entry|!matches!(&entry.pass,passes::Pass::Build(builder) if matches!(builder.name(),"experimental-compile-instant-events"|"experimental-compile-bud-memory-reads")));
    passes::run_graph_pipeline(&mut prepared.graph, &options, &prepared.entry, &pipeline, &monitor).unwrap();
    let graph = &mut prepared.graph;
    let reads = infer_bud_memory_reads(graph, &monitor).unwrap();
    assert_eq!(reads.len(), 16);
    let read = reads[0];
    graph.power_links.push(read.link);
    assert_eq!(infer_bud_memory_reads(graph, &monitor).unwrap().len(), 15);
    graph.power_links.pop();
    let link = graph.power_links.iter().position(|link| *link == read.link).unwrap();
    let extra = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(StateTest::PistonExtended(read.storage)));
    let conditional = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::All(vec![read.link.guard, extra].into_boxed_slice()));
    graph.power_links[link].guard = conditional;
    graph.contacts[read.source.index()].variants[0].when = conditional;
    assert_eq!(infer_bud_memory_reads(graph, &monitor).unwrap().len(), 15);
    monitor.cancel();
    assert!(infer_bud_memory_reads(graph, &monitor).is_err());
}

fn backend(lowered: &LoweredCircuit) -> GraphBackend {
    let mut backend = GraphBackend::new(lowered.program.clone(), lowered.state.clone(), 0).unwrap();
    backend.set_test_recording(true);
    backend
}

#[test]
fn counter_bud_memory_reads_match_general_queries_for_every_pose_and_publication_state() {
    let circuit = boundary_contract_tests::load_mechanism("counter_basic");
    let (baseline, compiled) = prepare(&circuit);
    let mut general = backend(&baseline);
    let mut candidate = backend(&compiled);
    let reads = candidate.program.bud_memory_reads().to_vec();
    assert_eq!(reads.len(), 16);
    for read in reads {
        let contacts = candidate.program.resources()[read.resource.index()].contacts.clone();
        let node = candidate.program.pistons()[read.reader.index()].node;
        for phase in [
            PistonAvailability::Ready,
            PistonAvailability::HeadPending,
            PistonAvailability::PublishingHeadRemoval,
            PistonAvailability::BasePending,
            PistonAvailability::BasePendingHeadPresent,
        ] {
            for flags in 0..8 {
                for &at in contacts.iter() {
                    for backend in [&mut general, &mut candidate] {
                        let actor = &mut backend.state.pistons[read.storage.index()];
                        actor.availability = phase;
                        actor.extended = flags & 1 != 0;
                        actor.head_present = flags & 2 != 0;
                        let payload = &mut backend.state.resources[read.resource.index()];
                        payload.at = at;
                        payload.enabled = flags & 4 != 0;
                    }
                    assert_eq!(
                        candidate.input_strength(node, InputPort::Quasi),
                        general.input_strength(node, InputPort::Quasi)
                    );
                }
            }
        }
    }
}

#[test]
fn adder11_compiles_one_stateful_kernel_and_preserves_all_pinned_histories() {
    check_adder11(false);
}

#[test]
fn cpu_kernels_adder11_preserves_all_pinned_histories() {
    check_adder11(true);
}

fn check_adder11(cpu: bool) {
    let circuit = load_adder("adder_11bits", ORIGIN, None);
    let (baseline, compiled) = prepare_mode(&circuit, cpu);
    if cpu {
        assert_eq!(compiled.program.cpu_kernels().entries.len(), 142);
    }
    assert_eq!(compiled.program.instant_kernels().len(), 1);
    let kernel = &compiled.program.instant_kernels()[0];
    assert_eq!(kernel.actors.len(), 142);
    let KernelProgram::Events(entries) = &kernel.program else {
        panic!("expected stateful kernel")
    };
    assert_eq!(entries.len(), 142);
    assert!(kernel.wires.is_empty(), "retained propagation is not eliminated-wire coverage");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_11bits.json")).unwrap()).unwrap();
    let mut cases = 0;
    let mut invocations = 0;
    for case in manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["expectation"].is_object())
    {
        let mut native = circuit.build();
        let mut projected = circuit.build();
        let mut general = backend(&baseline);
        let mut candidate = backend(&compiled);
        let mut steps = case_steps(&circuit, case, false, true);
        let response = steps.len();
        steps.extend(std::iter::repeat_n(Step::Tick, 16));
        for (index, step) in steps.into_iter().enumerate() {
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever {lever} if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut general, step, toggle).unwrap();
            graph_step(&mut candidate, step, toggle).unwrap();
            instant_proxy_tests::assert_same_memory(&general, &candidate);
            check_boundary(
                &candidate,
                &native,
                &mut projected,
                &circuit.positions(),
                &format!("event adder11 {} step {index}", case["id"]),
            )
            .unwrap();
            if index >= response {
                let expected = expectation(&circuit, case);
                assert_math_window(&circuit, &projected, expected.0, expected.1, index - response + 1);
            }
        }
        invocations += candidate.execution_counts().instant_kernel_invocations;
        cases += 1;
    }
    assert_eq!(cases, 40);
    assert!(invocations > 0, "the connected program must execute");
    let dir = root().join(if cpu { "target/cpu-kernels" } else { "target/instant-event-kernels" });
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("adder11.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "kernels":1,"actors":142,"entry_points":entries.len(),"pinned_histories":cases,"kernel_invocations":invocations,
            "wire_propagation_removed":false,"semantics":"one FIFO-local request per entry, original live wires and reset clocks"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn adder11_event_kernel_hands_back_partial_cycles_with_identical_future() {
    check_handoffs(false);
}

#[test]
fn cpu_kernels_adder11_hands_back_partial_cycles_with_identical_future() {
    check_handoffs(true);
}

fn check_handoffs(cpu: bool) {
    let circuit = load_adder("adder_11bits", ORIGIN, None);
    let (_, compiled) = prepare_mode(&circuit, cpu);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures/adder_11bits.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["expectation"]["carry"].as_u64() == Some(1))
        .unwrap();
    for cut in [0, 1, 2, 4, 8, 16] {
        let mut native = circuit.build();
        let mut handed_back = circuit.build();
        let mut candidate = backend(&compiled);
        let mut steps = case_steps(&circuit, case, false, true);
        steps.extend(std::iter::repeat_n(Step::Tick, cut));
        for step in steps {
            let toggle = match step {
                Step::SetLever { position, powered } => matches!(native.get_block(position), Block::Lever {lever} if lever.powered != powered),
                Step::Tick => false,
            };
            native_step(&mut native, step);
            graph_step(&mut candidate, step, toggle).unwrap();
        }
        candidate.reset(&mut handed_back);
        for tick in 0..17 {
            assert_eq!(
                first_difference(&future(&native, &circuit.positions()), &future(&handed_back, &circuit.positions())),
                None,
                "event adder11 cut {cut} continuation {tick}"
            );
            native.tick_interpreted();
            handed_back.tick_interpreted();
        }
    }
}

#[test]
fn cpu_kernels_live_queries_match_general_for_random_strengths_and_resource_poses() {
    for circuit in [
        load_adder("adder_11bits", ORIGIN, None),
        boundary_contract_tests::load_mechanism("counter_basic"),
    ] {
        let (baseline, compiled) = prepare_mode(&circuit, true);
        let mut general = backend(&baseline);
        let mut candidate = backend(&compiled);
        let mut seed = 0x91af_8891u32;
        let mut random = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed
        };
        for _ in 0..256 {
            for state in &mut general.state.components {
                match state {
                    ComponentState::Electrical(s) => {
                        s.output_strength = (random() % 16) as u8;
                        s.powered = random() & 1 != 0;
                        s.repeater_locked = random() & 1 != 0;
                    }
                    ComponentState::Wire(s) => s.power = (random() % 16) as u8,
                    _ => {}
                }
            }
            for state in &mut general.state.pistons {
                let bits = random();
                state.extended = bits & 1 != 0;
                state.head_present = bits & 2 != 0;
                state.availability = [
                    PistonAvailability::Ready,
                    PistonAvailability::HeadPending,
                    PistonAvailability::PublishingHeadRemoval,
                    PistonAvailability::BasePending,
                    PistonAvailability::BasePendingHeadPresent,
                ][random() as usize % 5];
            }
            for (id, state) in general.state.resources.iter_mut().enumerate() {
                let contacts = &general.program.resources()[id].contacts;
                state.at = contacts[random() as usize % contacts.len()];
                state.enabled = random() & 1 != 0;
            }
            candidate.state = general.state.clone();
            for entry in &candidate.program.cpu_kernels().entries {
                let node = general.program.pistons()[entry.actor.index()].node;
                assert_eq!(
                    candidate.cpu_predicate(entry.main),
                    general.input_strength(node, InputPort::Main) > 0,
                    "Main {:?}",
                    entry.actor
                );
                assert_eq!(
                    candidate.cpu_predicate(entry.quasi),
                    general.input_strength(node, InputPort::Quasi) > 0,
                    "Quasi {:?}",
                    entry.actor
                );
            }
        }
    }
}
