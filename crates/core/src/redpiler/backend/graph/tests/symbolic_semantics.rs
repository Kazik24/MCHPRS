//! Graph-derived symbolic reads are compared with the authoritative concrete evaluator.
use super::*;
use crate::redpiler::analysis::instant_regions::compiler::Compiler as SymbolicCompiler;
use crate::redpiler::program_graph::{Guard, PowerPort, PowerTarget, SiteSource, StateTest};
use mchprs_redpiler_opt::{
    Limits,
    symbolic::{Formula, Signal},
};

fn constant_signal(value: Signal) -> u8 {
    value.bits().iter().enumerate().fold(0, |result, (bit, value)| {
        result | (u8::from(value.constant().expect("a constant graph state must reduce to a constant signal")) << bit)
    })
}

fn targets(graph: &CompileGraph) -> Vec<PowerTarget> {
    let mut seen = FxHashSet::default();
    graph
        .power_links
        .iter()
        .filter_map(|link| seen.insert(link.to).then_some(link.to))
        .collect()
}

fn exact_power(backend: &GraphBackend, target: PowerTarget) -> u8 {
    match target {
        PowerTarget::Input { node, port } => backend.input_strength(node, port),
        PowerTarget::Conductor { site, include_dust } => backend.conducted_power(site, include_dust),
    }
}

#[test]
fn both_adder_graphs_compile_all_initial_guards_and_electrical_channels() {
    for id in FAMILY {
        let (graph, _) = stable_region_tests::stable_graph(id);
        let (program, state) = graph.clone().lower().unwrap();
        let mut exact = GraphBackend::new(program, state, 0).unwrap();
        let mut symbolic = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
        for index in 0..graph.guards.len() {
            let guard = GuardId::new(index as u32);
            assert_eq!(
                symbolic.guard(guard, None).unwrap().constant(),
                Some(exact.guard_matches(guard, None)),
                "{id} guard {index}"
            );
        }
        for target in targets(&graph) {
            assert_eq!(
                constant_signal(symbolic.query_power(target).unwrap()),
                exact_power(&exact, target),
                "{id} target {target:?}"
            );
        }
        for (index, site) in graph.sites.iter().enumerate() {
            let site_id = SiteId::new(index as u32);
            for port in [PowerPort::Stored, PowerPort::Output].into_iter().chain(
                BlockFace::values()
                    .into_iter()
                    .flat_map(|face| [PowerPort::Weak(face), PowerPort::Strong(face)]),
            ) {
                assert_eq!(
                    constant_signal(symbolic.site_power(site_id, port).unwrap()),
                    exact.site_power(site_id, port),
                    "{id} site {index} {port:?}"
                );
            }
            if matches!(site.source, SiteSource::Component(_)) {
                for face in BlockFace::values() {
                    assert_eq!(
                        constant_signal(symbolic.power_no_dust(site_id, face, None).unwrap()),
                        exact.power_no_dust(site_id, face, None),
                        "{id} no-dust {index} {face:?}"
                    );
                }
            }
        }
        // This read-only compiler must not enqueue work or change the authoritative runtime state.
        assert_eq!(exact.execution_counts().piston_actions, 0);
        exact.tick().unwrap();
    }
}

#[test]
fn eleven_bit_symbolic_channels_keep_all_controls_as_variables() {
    let (graph, _) = stable_region_tests::stable_graph("adder_11bits");
    let (program, state) = graph.clone().lower().unwrap();
    let mut exact = GraphBackend::new(program, state, 0).unwrap();
    let mut symbolic = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let controls: Vec<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            (node.kind == CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever)).then_some(NodeId::new(index as u32))
        })
        .collect();
    assert_eq!(controls.len(), 23, "all 22 data controls and activation remain symbolic");
    for (input, &node) in controls.iter().enumerate() {
        let variable = symbolic.logic.input(input).unwrap();
        symbolic.state.components[node.index()].powered = variable;
        symbolic.state.components[node.index()].signal = symbolic
            .logic
            .signal_select(variable, Signal::literal(15).unwrap(), Signal::literal(0).unwrap())
            .unwrap();
    }
    symbolic.state_written();
    let targets = targets(&graph);
    let mut roots = Vec::new();
    for &target in &targets {
        roots.extend(symbolic.query_power(target).unwrap().bits());
    }
    let circuit = symbolic.logic.circuit(&roots).unwrap();
    let plan = mchprs_redpiler_opt::optimize(&circuit, Limits::default(), || false).unwrap();
    assert_eq!(plan.inputs().len(), 23);
    let mut scratch = vec![false; plan.scratch_len()];
    let mut outputs = vec![false; roots.len()];
    // These are verification samples only; the compiler above visits no input masks.
    for sample in 0..27 {
        let inputs: Vec<_> = (0..controls.len())
            .map(|input| match sample {
                0 => false,
                1 => true,
                2 => input % 2 == 0,
                3 => input % 2 != 0,
                _ => input == sample - 4,
            })
            .collect();
        for (&node, &powered) in controls.iter().zip(&inputs) {
            let ComponentState::Electrical(value) = &mut exact.state.components[node.index()] else {
                unreachable!()
            };
            value.powered = powered;
            value.output_strength = u8::from(powered) * 15;
        }
        plan.evaluate(&inputs, &mut scratch, &mut outputs).unwrap();
        for (index, &target) in targets.iter().enumerate() {
            let result = outputs[index * 4..index * 4 + 4]
                .iter()
                .enumerate()
                .fold(0, |value, (bit, &set)| value | (u8::from(set) << bit));
            assert_eq!(result, exact_power(&exact, target), "sample {sample} target {target:?}");
        }
    }
}

#[test]
fn symbolic_captures_freeze_old_values_while_later_reads_observe_new_writes() {
    let (mut graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let reset = graph.plans.instant.regions[0].resets[0];
    let site = graph.sites.iter().position(|site| site.source == SiteSource::Component(reset)).unwrap();
    let site = SiteId::new(site as u32);
    let actor = graph.plans.instant.regions[0].actors[0];
    let guard = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(StateTest::PistonExtended(actor)));
    let mut symbolic = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let a = symbolic.logic.input(0).unwrap();
    let b = symbolic.logic.input(1).unwrap();
    for (powered, capture) in [(a, true), (b, false)] {
        symbolic.state.components[reset.index()].powered = powered;
        symbolic.state.components[reset.index()].signal = symbolic
            .logic
            .signal_select(powered, Signal::literal(15).unwrap(), Signal::literal(0).unwrap())
            .unwrap();
        symbolic.state.actors[actor.index()].extended = powered;
        symbolic.state_written();
        if capture {
            assert_eq!(symbolic.guard(guard, None).unwrap(), a);
        }
    }
    assert_eq!(symbolic.guard(guard, None).unwrap(), b, "the live guard cache cannot survive a write");
    symbolic.state.components[reset.index()].powered = a;
    symbolic.state_written();
    let captured = symbolic.observe(site).unwrap();
    symbolic.state.components[reset.index()].powered = b;
    symbolic.state_written();
    let block = Block::from_id(graph.nodes[reset.index()].raw_state.unwrap());
    let face = BlockFace::values()
        .into_iter()
        .find(|&face| crate::redpiler::power_rules::emits_weak(block, face))
        .unwrap();
    let captured_power = symbolic.power_no_dust(site, face, Some(&captured)).unwrap();
    let live_power = symbolic.power_no_dust(site, face, None).unwrap();
    let roots: Vec<_> = captured_power.bits().into_iter().chain(live_power.bits()).collect();
    let plan = mchprs_redpiler_opt::optimize(&symbolic.logic.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
    for inputs in [[false, false], [false, true], [true, false], [true, true]] {
        let mut output = [false; 8];
        plan.evaluate(&inputs, &mut vec![false; plan.scratch_len()], &mut output).unwrap();
        assert_eq!(output[..4], [inputs[0]; 4]);
        assert_eq!(output[4..], [inputs[1]; 4]);
    }
    assert_ne!(a, Formula::FALSE);
}

#[test]
fn both_adders_compile_seed_and_captured_dust_queries_over_symbolic_strengths() {
    use crate::redpiler::analysis::instant_regions::compiler::Capture;
    for id in FAMILY {
        let (graph, _) = stable_region_tests::stable_graph(id);
        let (program, state) = graph.clone().lower().unwrap();
        let mut exact = GraphBackend::new(program, state, 0).unwrap();
        let mut symbolic = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
        let strength = Signal::from_bits(std::array::from_fn(|bit| symbolic.logic.input(bit).unwrap()));
        for wire in &graph.wires {
            symbolic.state.components[wire.node.index()].signal = strength;
        }
        symbolic.state_written();
        let mut capture = Capture::default();
        for wire in &graph.wires {
            for site in wire.canonical_neighbors {
                if !capture.contains_key(&site) {
                    capture.insert(site, symbolic.observe(site).unwrap());
                }
            }
        }
        let mut roots = Vec::new();
        for wire in &graph.wires {
            roots.extend(symbolic.seed_power(wire.node).unwrap().bits());
            for heading in 0..4 {
                roots.extend(symbolic.captured_wire_power(wire.node, &capture, heading).unwrap().bits());
            }
        }
        let plan = mchprs_redpiler_opt::optimize(&symbolic.logic.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
        let mut scratch = vec![false; plan.scratch_len()];
        let mut outputs = vec![false; roots.len()];
        for strength in 0u8..16 {
            for wire in &graph.wires {
                let ComponentState::Wire(value) = &mut exact.state.components[wire.node.index()] else {
                    unreachable!()
                };
                value.power = strength;
            }
            let values = std::array::from_fn::<_, 4, _>(|bit| strength & (1 << bit) != 0);
            plan.evaluate(&values, &mut scratch, &mut outputs).unwrap();
            for (index, wire) in graph.wires.iter().enumerate() {
                let read = |offset| {
                    outputs[(index * 5 + offset) * 4..(index * 5 + offset + 1) * 4]
                        .iter()
                        .enumerate()
                        .fold(0, |value, (bit, &set)| value | (u8::from(set) << bit))
                };
                assert_eq!(
                    read(0),
                    exact.seed_power_general(wire).0,
                    "{id} seed {:?}, strength {strength}",
                    wire.node
                );
                for (heading, (x, z)) in [(0, -1), (1, 0), (0, 1), (-1, 0)].into_iter().enumerate() {
                    let mut walk = WireWalk::new();
                    let site = exact.component_sites[wire.node.index()].unwrap();
                    let observed = exact.resolve_site(site, None).unwrap().observed();
                    walk.insert_site(
                        site,
                        WalkSite {
                            site,
                            observed,
                            visited: true,
                            x_bias: x,
                            z_bias: z,
                            layer: 0,
                            heading: None,
                            neighbor_slots: None,
                        },
                    );
                    let neighbors = exact.identify_wire_neighbors(wire.node, &mut walk);
                    assert_eq!(
                        read(heading + 1),
                        exact.turbo_wire_power_general(wire, &neighbors, &walk).0,
                        "{id} captured {:?}, heading {heading}, strength {strength}",
                        wire.node
                    );
                }
            }
        }
    }
}

#[test]
fn symbolic_shape_transitions_match_every_legal_pair_and_neighbor_direction() {
    use crate::redpiler::analysis::instant_regions::compiler::{WireShape, WireSide};
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let mut symbolic = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let mut shape = |first| WireShape {
        sides: std::array::from_fn(|side| WireSide {
            up: symbolic.logic.input(first + side * 2).unwrap(),
            flat: symbolic.logic.input(first + side * 2 + 1).unwrap(),
        }),
    };
    let stored = shape(0);
    let raw = shape(8);
    let mut roots = Vec::new();
    for face in BlockFace::values() {
        let result = symbolic.neighbor_shape(stored, raw, face).unwrap();
        for side in result.sides {
            roots.extend([side.up, side.flat]);
        }
    }
    let plan = mchprs_redpiler_opt::optimize(&symbolic.logic.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
    let mut scratch = vec![false; plan.scratch_len()];
    let mut outputs = vec![false; roots.len()];
    for case in 0..3usize.pow(8) {
        let mut code = case;
        let mut values = [false; 16];
        let mut sides = [mchprs_blocks::blocks::RedstoneWireSide::None; 8];
        for (index, side) in sides.iter_mut().enumerate() {
            *side = match code % 3 {
                0 => mchprs_blocks::blocks::RedstoneWireSide::None,
                1 => {
                    values[index * 2] = true;
                    mchprs_blocks::blocks::RedstoneWireSide::Up
                }
                2 => {
                    values[index * 2 + 1] = true;
                    mchprs_blocks::blocks::RedstoneWireSide::Side
                }
                _ => unreachable!(),
            };
            code /= 3;
        }
        let wire = |offset| mchprs_blocks::blocks::RedstoneWire {
            north: sides[offset],
            south: sides[offset + 1],
            east: sides[offset + 2],
            west: sides[offset + 3],
            power: 7,
        };
        plan.evaluate(&values, &mut scratch, &mut outputs).unwrap();
        for (index, face) in BlockFace::values().into_iter().enumerate() {
            let expected = crate::redstone::wire::on_neighbor_changed_from(wire(0), wire(4), face);
            let expected = [expected.north, expected.south, expected.east, expected.west];
            for (side, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    &outputs[index * 8 + side * 2..index * 8 + side * 2 + 2],
                    [
                        expected == mchprs_blocks::blocks::RedstoneWireSide::Up,
                        expected == mchprs_blocks::blocks::RedstoneWireSide::Side
                    ],
                    "case {case} direction {face:?} side {side}"
                );
            }
        }
    }
}

#[test]
fn symbolic_shared_payload_writes_preserve_identity_revisions_and_round_trip_invalidation() {
    let (mut graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let resource = graph.resources.iter().position(|resource| resource.contacts.len() >= 2).unwrap();
    graph.initial_resources[resource].revision = u64::MAX;
    let resource = ResourceId::new(resource as u32);
    let initial = graph.initial_resources[resource.index()];
    let other = graph.resources[resource.index()]
        .contacts
        .iter()
        .copied()
        .find(|&contact| contact != initial.at)
        .unwrap();
    let mut compiler = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let a = compiler.logic.input(0).unwrap();
    let b = compiler.logic.input(1).unwrap();
    let before = compiler.state.resources[resource.index()].revision.stamp();
    compiler.write_resource(resource, other, false, a).unwrap();
    let during = compiler.state.resources[resource.index()].revision.stamp();
    compiler.write_resource(resource, other, true, b).unwrap();
    compiler.write_resource(resource, initial.at, initial.enabled, a).unwrap();
    let state = &compiler.state.resources[resource.index()];
    let unchanged_before = state.revision.unchanged(before, &mut compiler.logic).unwrap();
    let unchanged_during = state.revision.unchanged(during, &mut compiler.logic).unwrap();
    let mut roots: Vec<_> = state.contacts.iter().map(|(_, at)| *at).collect();
    roots.extend([state.enabled, unchanged_before, unchanged_during]);
    roots.extend(state.revision.writes.iter().copied());
    let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
    for inputs in [[false, false], [false, true], [true, false], [true, true]] {
        let mut expected = initial;
        let mut intermediate_revision = initial.revision;
        for (index, (contact, enabled, when)) in [
            (other, false, inputs[0]),
            (other, true, inputs[1]),
            (initial.at, initial.enabled, inputs[0]),
        ]
        .into_iter()
        .enumerate()
        {
            if when && (expected.at != contact || expected.enabled != enabled) {
                expected.at = contact;
                expected.enabled = enabled;
                expected.revision = expected.revision.wrapping_add(1);
            }
            if index == 0 {
                intermediate_revision = expected.revision;
            }
        }
        let mut output = vec![false; roots.len()];
        plan.evaluate(&inputs, &mut vec![false; plan.scratch_len()], &mut output).unwrap();
        let contacts = state.contacts.len();
        assert_eq!(
            output[..contacts].iter().filter(|&&at| at).count(),
            1,
            "one resource cannot split into independent piston bits"
        );
        for (index, &(contact, _)) in state.contacts.iter().enumerate() {
            assert_eq!(output[index], expected.at == contact);
        }
        assert_eq!(
            output[contacts..contacts + 3],
            [
                expected.enabled,
                expected.revision == initial.revision,
                expected.revision == intermediate_revision
            ]
        );
        let increments = output[contacts + 3..].iter().filter(|&&write| write).count() as u64;
        assert_eq!(
            state.revision.base.wrapping_add(increments),
            expected.revision,
            "wrapping cold projection"
        );
    }
}

#[test]
fn guarded_actor_writes_preserve_one_discrete_pose_and_invalidate_later_live_queries() {
    let (graph, _) = stable_region_tests::stable_graph("adder_1bit");
    let actor = PistonId::new(0);
    let initial = graph.initial_pistons[0];
    let mut compiler = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let a = compiler.logic.input(0).unwrap();
    let b = compiler.logic.input(1).unwrap();
    let first = compiler.revise_actor(actor, a).unwrap();
    compiler
        .write_actor(actor, a, Some(Formula::TRUE), Some(Formula::TRUE), Some(PistonAvailability::HeadPending))
        .unwrap();
    compiler.revise_actor(actor, b).unwrap();
    compiler
        .write_actor(actor, b, None, Some(Formula::FALSE), Some(PistonAvailability::BasePending))
        .unwrap();
    let state = &compiler.state.actors[actor.index()];
    let unchanged = state.revision.unchanged(first, &mut compiler.logic).unwrap();
    let roots: Vec<_> = [state.extended, state.head, unchanged].into_iter().chain(state.availability).collect();
    let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
    for inputs in [[false, false], [false, true], [true, false], [true, true]] {
        let mut output = vec![false; roots.len()];
        plan.evaluate(&inputs, &mut vec![false; plan.scratch_len()], &mut output).unwrap();
        assert_eq!(output[0], initial.extended || inputs[0]);
        assert_eq!(output[1], !inputs[1] && (initial.head_present || inputs[0]));
        assert_eq!(output[2], !inputs[1], "the second accepted action invalidates the first witness");
        let expected = if inputs[1] {
            PistonAvailability::BasePending
        } else if inputs[0] {
            PistonAvailability::HeadPending
        } else {
            initial.availability
        };
        assert_eq!(output[3..].iter().filter(|&&value| value).count(), 1);
        for (index, &selected) in output[3..].iter().enumerate() {
            assert_eq!(selected, index == expected as usize);
        }
    }
}
