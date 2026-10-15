//! Entry wire values are functions of logical memory and sources, not saved wire strengths.
use super::*;
use crate::redpiler::analysis::instant_regions::compiler::execution::Execution;

fn activation(graph: &CompileGraph, position: BlockPos) -> NodeId {
    let index = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(position) && node.kind == CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever))
        .unwrap();
    NodeId::new(index as u32)
}

#[test]
fn local_shape_memories_cover_native_updates_across_sampled_profile_alternatives() {
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let (graph, _) = stable_region_tests::stable_graph_at_level(id, 1);
        let mut compiler = SymbolicCompiler::new(
            &graph,
            Limits {
                max_work: 256 * 1024 * 1024,
                ..Limits::default()
            },
        )
        .unwrap();
        let entry = Entry::derive(&mut compiler, activation(&graph, circuit.trigger())).unwrap();
        let mut random = 0x9387_a19du32;
        for (memory, wire) in entry.shapes.iter().zip(&graph.wires) {
            assert_eq!(memory.node, wire.node);
            assert!(!memory.values.is_empty() && memory.values.len() <= 81);
            for case in 0..256 {
                let neighbors = wire.canonical_neighbors.map(|site| {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    match graph.sites[site.index()].source {
                        SiteSource::Component(node) => Block::from_id(graph.nodes[node.index()].raw_state.unwrap()),
                        SiteSource::Constant(profile) => Block::from_id(graph.profiles[profile.index()].raw_state),
                        SiteSource::Contact(contact) => {
                            let variants = &graph.contacts[contact.index()].variants;
                            let variant = &variants[(random >> 16) as usize % variants.len()];
                            Block::from_id(graph.profiles[variant.profile.index()].raw_state)
                        }
                    }
                });
                let value = memory.values[case % memory.values.len()];
                let current = mchprs_blocks::blocks::RedstoneWire {
                    north: value[0],
                    south: value[1],
                    east: value[2],
                    west: value[3],
                    power: 9,
                };
                let raw = crate::redstone::wire::get_raw_sides_from_neighbors(current, &neighbors);
                for face in BlockFace::values() {
                    let updated = crate::redstone::wire::on_neighbor_changed_from(current, raw, face);
                    assert!(
                        memory.values.contains(&[updated.north, updated.south, updated.east, updated.west]),
                        "{id} {:?}, geometry case {case}, direction {face:?}",
                        wire.node
                    );
                }
            }
        }
        let fixed = entry.shapes.iter().filter(|memory| memory.values.len() == 1).count();
        assert!(fixed > 0, "fixed shape histories need no input register");
    }
}
use crate::redpiler::analysis::instant_regions::compiler::{Compiler as SymbolicCompiler, entry::Entry};
use mchprs_redpiler_opt::Limits;

#[test]
fn ready_entry_retains_quasi_only_actor_memory_and_rejects_missing_activation() {
    let circuit = load_adder("adder_1bit", ORIGIN, None);
    let (mut graph, _) = stable_region_tests::stable_graph_at_level("adder_1bit", 1);
    let actor = PistonId::new(0);
    let node = graph.pistons[actor.index()].node;
    graph
        .power_links
        .retain(|link| link.to != PowerTarget::Input { node, port: InputPort::Main });
    let mut compiler = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    assert!(matches!(Entry::derive(&mut compiler, NodeId::new(u32::MAX)), Err(_)));
    let entry = Entry::derive(&mut compiler, activation(&graph, circuit.trigger())).unwrap();
    let variable = entry
        .inputs
        .iter()
        .position(|input| *input == crate::redpiler::analysis::instant_regions::compiler::entry::Input::Extended(actor))
        .unwrap();
    let memory = compiler.logic.input(variable).unwrap();
    assert_eq!(
        compiler.state.actors[actor.index()].extended,
        memory,
        "QC-only extension is retained BUD memory"
    );
    assert_eq!(compiler.state.actors[actor.index()].head, memory);
}

#[test]
fn both_adder_ready_relations_reconstruct_every_prepared_fixture_without_wire_strength_inputs() {
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let (graph, _) = stable_region_tests::stable_graph_at_level(id, 1);
        let limits = Limits {
            max_work: 256 * 1024 * 1024,
            ..Limits::default()
        };
        let mut compiler = SymbolicCompiler::new(&graph, limits).unwrap();
        let entry = Entry::derive(&mut compiler, activation(&graph, circuit.trigger())).unwrap_or_else(|error| {
            panic!(
                "{id} entry {error:?}, decisions={}, work={}",
                compiler.logic.node_count(),
                compiler.logic.work()
            )
        });
        let mut roots = entry.conditions.clone();
        let state_offset = roots.len();
        roots.extend(symbolic_execution_tests::state_roots(&compiler));
        let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&roots).unwrap(), limits, || false).unwrap();
        let mut scratch = vec![false; plan.scratch_len()];
        let mut output = vec![false; roots.len()];
        let mut cases = 0;
        for case in manifest["cases"].as_array().unwrap() {
            if !case["expectation"].is_object() {
                continue;
            }
            let (program, state) = graph.clone().lower().unwrap();
            let mut exact = GraphBackend::new(program, state, 0).unwrap();
            for step in case_steps(&circuit, case, false, false) {
                match step {
                    Step::Tick => exact.tick().unwrap(),
                    Step::SetLever { position, powered } => {
                        let node = exact.positions[&position];
                        if exact.node_state(node).powered != powered {
                            exact.use_block(position).unwrap();
                        }
                    }
                }
            }
            assert!(
                exact.future.pending.is_empty() && exact.future.piston_requests.is_empty() && exact.future.deferred_pistons.is_empty(),
                "{id}/{} must reach a stationary, work-free activation boundary",
                case["id"]
            );
            let values = entry.read_inputs(&graph, &exact.state).unwrap();
            plan.evaluate(&values, &mut scratch, &mut output).unwrap();
            assert!(
                output[..state_offset].iter().all(|&valid| valid),
                "{id}/{} violates occupancy or fixed-point entry conditions",
                case["id"]
            );
            let expected = symbolic_execution_tests::exact_state(&exact);
            let mismatch = output[state_offset..].iter().zip(&expected).position(|(a, b)| a != b);
            if let Some(bit) = mismatch {
                let mut offset = 0;
                for (node, spec) in graph.nodes.iter().enumerate() {
                    let size = if matches!(spec.kind, CircuitNodeKind::Wire(_)) { 14 } else { 6 };
                    if (offset..offset + size).contains(&bit) {
                        panic!(
                            "{id}/{} node {node} {:?} {:?}, offset {offset}, expected {:?}, actual {:?}",
                            case["id"],
                            spec.kind,
                            spec.position,
                            &expected[offset..offset + size],
                            &output[state_offset + offset..state_offset + offset + size]
                        );
                    }
                    offset += size;
                }
            }
            assert!(mismatch.is_none(), "{id}/{} first differing prepared-state bit {mismatch:?}", case["id"]);
            cases += 1;
        }
        assert_eq!(cases, if id == "adder_1bit" { 8 } else { 40 });
        eprintln!(
            "{id}: ready inputs={}, decisions={}, pure operations={}, verified prepared states={cases}",
            entry.inputs.len(),
            compiler.logic.node_count(),
            plan.ops().len()
        );
    }
}

#[test]
fn symbolic_response_and_reset_reuse_one_program_after_changed_operands_with_exact_future_and_interpreter_outputs() {
    use super::symbolic_observation_tests::Observation;
    for id in FAMILY {
        let circuit = load_adder(id, ORIGIN, None);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root().join("test_data/instant-pistons-io/fixtures").join(format!("{id}.json"))).unwrap()).unwrap();
        let (graph, _) = stable_region_tests::stable_graph_at_level(id, 1);
        let limits = Limits {
            max_work: 256 * 1024 * 1024,
            ..Limits::default()
        };
        let mut compiler = SymbolicCompiler::new(&graph, limits).unwrap();
        let entry = Entry::derive(&mut compiler, activation(&graph, circuit.trigger())).unwrap();
        let mut roots = entry.conditions.clone();
        let entry_state_start = roots.len();
        roots.extend(symbolic_execution_tests::state_roots(&compiler));
        let entry_state = entry_state_start..roots.len();
        let trigger = NodeId::new(
            graph
                .nodes
                .iter()
                .position(|node| {
                    node.position == Some(circuit.trigger()) && node.kind == CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever)
                })
                .unwrap() as u32,
        );
        let mut execution = Execution::empty(&graph);
        execution
            .set_control(&mut compiler, trigger, mchprs_redpiler_opt::symbolic::Formula::FALSE)
            .unwrap();
        let mut frames = Vec::new();
        for tick in 1..=32 {
            if tick == 17 {
                execution
                    .set_control(&mut compiler, trigger, mchprs_redpiler_opt::symbolic::Formula::TRUE)
                    .unwrap();
            }
            let first_effect = execution.accepted.len();
            if let Err(error) = execution.run_tick(&mut compiler) {
                panic!(
                    "{id} generic ready response tick {tick}: {error:?}; decisions={}, work={}",
                    compiler.logic.node_count(),
                    compiler.logic.work()
                );
            }
            frames.push(Observation::capture(&compiler, &execution, first_effect, &mut roots));
        }
        let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&roots).unwrap(), limits, || false).unwrap();
        let mut scratch = vec![false; plan.scratch_len()];
        let mut output = vec![false; roots.len()];
        for case in manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["expectation"].is_object())
        {
            let (program, state) = graph.clone().lower().unwrap();
            let mut exact = GraphBackend::new(program, state, 0).unwrap();
            let mut native = circuit.build();
            let mut scratch_world = circuit.build();
            let positions = circuit.positions();
            for step in case_steps(&circuit, case, false, false) {
                native_step(&mut native, step);
                match step {
                    Step::Tick => exact.tick().unwrap(),
                    Step::SetLever { position, powered } => {
                        let node = exact.positions[&position];
                        if exact.node_state(node).powered != powered {
                            exact.use_block(position).unwrap();
                        }
                    }
                }
            }
            for transaction in 0..2 {
                if transaction != 0 {
                    for (index, node) in graph.nodes.iter().enumerate() {
                        if node.kind == CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever)
                            && NodeId::new(index as u32) != trigger
                        {
                            exact.use_block(node.position.unwrap()).unwrap();
                            native_step(
                                &mut native,
                                Step::SetLever {
                                    position: node.position.unwrap(),
                                    powered: exact.node_state(NodeId::new(index as u32)).powered,
                                },
                            );
                        }
                    }
                    for _ in 0..32 {
                        exact.tick().unwrap();
                        native_step(&mut native, Step::Tick);
                    }
                }
                let values = entry.read_inputs(&graph, &exact.state).unwrap();
                plan.evaluate(&values, &mut scratch, &mut output).unwrap();
                assert!(
                    output[..entry_state_start].iter().all(|&valid| valid),
                    "{id}/{} ready admission {transaction}",
                    case["id"]
                );
                assert_eq!(
                    output[entry_state.clone()],
                    symbolic_execution_tests::exact_state(&exact),
                    "{id}/{} ready projection {transaction}",
                    case["id"]
                );
                let origins: Vec<_> = exact
                    .state
                    .pistons
                    .iter()
                    .map(|actor| actor.revision)
                    .chain(exact.state.resources.iter().map(|resource| resource.revision))
                    .collect();
                exact.use_block(circuit.trigger()).unwrap();
                native_step(
                    &mut native,
                    Step::SetLever {
                        position: circuit.trigger(),
                        powered: false,
                    },
                );
                for (tick, frame) in frames.iter().enumerate() {
                    if tick == 16 {
                        exact.use_block(circuit.trigger()).unwrap();
                        native_step(
                            &mut native,
                            Step::SetLever {
                                position: circuit.trigger(),
                                powered: true,
                            },
                        );
                    }
                    let first_effect = exact.effects.len();
                    exact.tick().unwrap();
                    native_step(&mut native, Step::Tick);
                    frame.assert_matches(&output, &exact, &origins, first_effect);
                    check_boundary(
                        &exact,
                        &native,
                        &mut scratch_world,
                        &positions,
                        &format!("{id}/{} transaction {transaction}, phase {tick}", case["id"]),
                    )
                    .unwrap();
                    if transaction == 0 && tick < 16 {
                        let (sum, carry) = expectation(&circuit, case);
                        assert_math_window(&circuit, &native, sum, carry, tick + 1);
                    }
                }
                assert!(
                    exact.future.pending.is_empty() && exact.future.piston_requests.is_empty() && exact.future.deferred_pistons.is_empty(),
                    "{id}/{} reset must reach an empty future",
                    case["id"]
                );
            }
        }
        eprintln!(
            "{id}: parameterized memory response decisions={}, ops={}, frames={}",
            compiler.logic.node_count(),
            plan.ops().len(),
            frames.len()
        );
    }
}
