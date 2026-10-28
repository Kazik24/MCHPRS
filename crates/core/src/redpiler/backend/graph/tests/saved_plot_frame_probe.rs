//! Corrected saved-plot diagnostic; six-tick samples are not proven settled.
//! This steps a `SettledMachine` every game tick and compares logical dust
//! outputs with native repeaters, so it does not reproduce production dispatch.
//! It samples the interpreted world each tick and carries feedback between steps;
//! the production frame samples at compute/resample and converges reset feedback.
use super::*;
use crate::redpiler::analysis::{AnalysisLimits, instant::layers::logical::compile_computation};
use crate::redpiler::components::NodeType;
use crate::redpiler::passes;
use crate::redpiler::program_graph::{LayerInputSource, PowerPort, PowerTarget, SettledTarget, SiteSource, StateTest};

#[test]
#[ignore = "corrected saved plot comparison; six-tick boundaries are not proven settled"]
fn corrected_fpu_logical_frame_probe() {
    let mut world = corrected_saved_world();
    let manifest = saved_fpu_manifest();
    let ports = &manifest["ports"];
    let inputs = ["light_blue_operand_inputs", "red_operand_inputs", "opcode_levers_by_sign_weight_4_2_1"]
        .into_iter()
        .flat_map(|key| manifest_positions(&ports[key]))
        .collect::<Vec<_>>();
    let dust = manifest_positions(&ports["output_by_label_bit_15_to_0"]);
    let readouts = dust.iter().map(|p| p.offset(BlockFace::East)).collect::<Vec<_>>();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
        ..Default::default()
    };
    let mut prepared = passes::rich::prepare(
        &world,
        world.get_corners(),
        world.scheduler().iter_entries().collect(),
        &monitor,
        AnalysisLimits::for_budget(8),
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
    let graph = prepared.graph;
    let targets = dust
        .iter()
        .map(|p| {
            let index = graph.nodes.iter().position(|n| n.position == Some(*p)).expect("output dust node");
            PowerTarget::Input {
                node: NodeId::new(index as u32),
                port: InputPort::Main,
            }
        })
        .collect::<Vec<_>>();
    let circuit = compile_computation(&graph, &targets, &monitor).expect("extract saved FPU computation");
    assert!(circuit.declines.is_empty(), "computation declines: {:?}", circuit.declines);
    let feedback = circuit
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(i, o)| match o.target {
            SettledTarget::ResourceState { resource, source } => Some((resource, source, i)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let sample = |source| match source {
        LayerInputSource::SourceAbove { site, port, threshold } => {
            let site = &graph.sites[site.index()];
            match site.source {
                SiteSource::Constant(_) => site.source_strength > threshold,
                SiteSource::Component(node) => {
                    assert!(
                        port == PowerPort::Output
                            && matches!(
                                graph.nodes[node.index()].kind,
                                CircuitNodeKind::Electrical(NodeType::Lever | NodeType::Button | NodeType::PressurePlate)
                            ),
                        "unsupported external source {:?}",
                        graph.nodes[node.index()]
                    );
                    let pos = graph.nodes[node.index()].position.unwrap();
                    crate::redstone::source_strength(world.get_block(pos), &world, pos) > threshold
                }
                other => panic!("unsupported source {other:?}"),
            }
        }
        LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => graph.initial_resources[resource.index()].at == contact,
        other => panic!("unsupported input {other:?}"),
    };
    let initial = circuit.program.inputs.iter().map(|input| sample(input.source)).collect::<Vec<_>>();
    let mut machine = crate::redpiler::program_graph::SettledMachine::new(&circuit, &initial).unwrap();
    let output_indices = targets
        .iter()
        .map(|target| circuit.outputs.iter().position(|o| o.target == SettledTarget::Power(*target)).unwrap())
        .collect::<Vec<_>>();
    // Bit 11 differs at the held native readout; list its transitive equation leaves.
    let mut stack = circuit.outputs[output_indices[4]].roots.to_vec();
    let mut seen = std::collections::BTreeSet::new();
    let mut leaves = std::collections::BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.0) {
            continue;
        }
        match &circuit.program.expressions[id.0 as usize] {
            crate::redpiler::program_graph::LayerExpr::Input(input) => {
                leaves.insert(input.0);
            }
            crate::redpiler::program_graph::LayerExpr::Not(child) => stack.push(*child),
            crate::redpiler::program_graph::LayerExpr::All(children) | crate::redpiler::program_graph::LayerExpr::Any(children) => {
                stack.extend(children.iter().copied())
            }
            crate::redpiler::program_graph::LayerExpr::Constant(_) => {}
        }
    }
    for index in leaves {
        let source = circuit.program.inputs[index as usize].source;
        match source {
            LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => {
                let owners = graph
                    .plans
                    .instant
                    .boundaries
                    .as_ref()
                    .unwrap()
                    .cells
                    .iter()
                    .flat_map(|cell| cell.resources.iter())
                    .filter(|pin| pin.resource == resource)
                    .flat_map(|pin| pin.owners.iter())
                    .map(|actor| (*actor, graph.pistons[actor.index()].position))
                    .collect::<Vec<_>>();
                println!(
                    "bit 11 feedback {resource:?} at {:?} owners={owners:?}",
                    graph.contacts[contact.index()].position
                );
            }
            LayerInputSource::SourceAbove { site, .. } => {
                if let SiteSource::Component(node) = graph.sites[site.index()].source {
                    println!("bit 11 source {source:?} at {:?}", graph.nodes[node.index()].position);
                }
            }
            _ => println!("bit 11 source {source:?}"),
        }
    }
    let vectors = [
        0,
        0x3_ffff_0000,
        0x4_1234_5678,
        0x7_abcd_ef01,
        0x1_0000_ffff,
        0x6_ffff_0000,
        0x2_5555_aaaa,
        0x5_aaaa_5555,
    ];
    let mut previous = inputs.iter().enumerate().fold(0u64, |word, (bit, &p)| {
        word | (u64::from(matches!(world.get_block(p), Block::Lever { lever } if lever.powered)) << bit)
    });
    for (bit, &p) in inputs.iter().enumerate() {
        if (previous ^ vectors[0]) & (1 << bit) != 0 {
            native_use_lever(&mut world, p);
        }
    }
    previous = vectors[0];
    native_use_lever(&mut world, TRIGGER);
    let mut results = Vec::new();
    let mut history = Vec::new();
    let mut held = Vec::new();
    for tick in 0..(24 + 6 * 3 + 48) {
        if tick >= 24 && tick < 24 + 6 * 3 && (tick - 24) % 6 == 0 {
            let word = vectors[(tick - 24) / 6];
            for (bit, &p) in inputs.iter().enumerate() {
                if (previous ^ word) & (1 << bit) != 0 {
                    native_use_lever(&mut world, p);
                }
            }
            previous = word;
        }
        world.tick_interpreted();
        let external = machine
            .external_inputs()
            .iter()
            .map(|input| {
                let LayerInputSource::SourceAbove { site, port, threshold } = input.source else {
                    panic!("unsupported external input {:?}", input.source)
                };
                let source = &graph.sites[site.index()];
                match source.source {
                    SiteSource::Constant(_) => source.source_strength > threshold,
                    SiteSource::Component(node) => {
                        assert_eq!(port, PowerPort::Output);
                        let pos = graph.nodes[node.index()].position.unwrap();
                        crate::redstone::source_strength(world.get_block(pos), &world, pos) > threshold
                    }
                    other => panic!("unsupported source {other:?}"),
                }
            })
            .collect::<Vec<_>>();
        machine.step(&external).unwrap();
        history.push(external);
        if tick >= 24 && tick < 24 + 6 * 3 && (tick - 24) % 6 == 5 {
            let native = readouts.iter().fold(0u16, |w, p| {
                (w << 1) | u16::from(matches!(world.get_block(*p), Block::RedstoneRepeater { repeater } if repeater.powered))
            });
            let logical = output_indices.iter().fold(0u16, |w, &i| (w << 1) | u16::from(machine.outputs()[i] != 0));
            results.push((vectors[(tick - 24) / 6], native, logical));
        }
        if tick >= 24 + 6 * 2 {
            let wire = dust.iter().fold(0u16, |w, p| {
                (w << 1) | u16::from(matches!(world.get_block(*p), Block::RedstoneWire { wire } if wire.power > 0))
            });
            let repeater = readouts.iter().fold(0u16, |w, p| {
                (w << 1) | u16::from(matches!(world.get_block(*p), Block::RedstoneRepeater { repeater } if repeater.powered))
            });
            let logical = output_indices.iter().fold(0u16, |w, &i| (w << 1) | u16::from(machine.outputs()[i] != 0));
            held.push((tick - (24 + 6 * 2) + 1, wire, repeater, logical));
        }
    }
    println!(
        "six-tick boundary samples (not proven settled), feedback={} (input,native,logical): {results:016x?}",
        feedback.len()
    );
    println!(
        "held vector 0x412345678 tick, native dust, native repeater, logical dust: first cycle={:04x?}; later boundaries={:04x?}",
        &held[..6],
        held.iter().skip(11).step_by(6).collect::<Vec<_>>()
    );
    assert_eq!(results.iter().map(|r| r.1).collect::<Vec<_>>(), [0xffff, 0, 0x8800]);
    let mut replay = crate::redpiler::program_graph::SettledMachine::new(&circuit, &initial).unwrap();
    for external in &history {
        replay.step(external).unwrap();
    }
    assert_eq!(replay.input_values(), machine.input_values(), "fresh replay must reconstruct feedback");
    assert_eq!(replay.outputs(), machine.outputs(), "fresh replay must reconstruct output");
    machine = replay;
    let mut samples = machine
        .external_inputs()
        .iter()
        .map(|input| {
            let LayerInputSource::SourceAbove { site, threshold, .. } = input.source else {
                panic!("unsupported external input")
            };
            let source = &graph.sites[site.index()];
            match source.source {
                SiteSource::Constant(_) => source.source_strength > threshold,
                SiteSource::Component(node) => {
                    let pos = graph.nodes[node.index()].position.unwrap();
                    crate::redstone::source_strength(world.get_block(pos), &world, pos) > threshold
                }
                other => panic!("unsupported source {other:?}"),
            }
        })
        .collect::<Vec<_>>();
    let bindings = machine
        .external_inputs()
        .iter()
        .enumerate()
        .filter_map(|(slot, input)| {
            let LayerInputSource::SourceAbove { site, threshold, .. } = input.source else {
                return None;
            };
            let SiteSource::Component(node) = graph.sites[site.index()].source else {
                return None;
            };
            let pos = graph.nodes[node.index()].position?;
            inputs.iter().position(|p| *p == pos).map(|bit| (slot, bit, threshold))
        })
        .collect::<Vec<_>>();
    let iterations = 10_000u64;
    let mut persistent_words = Vec::with_capacity(iterations as usize);
    let mut random = 0x19a4110b5eedu64;
    let mut checksum = 0u16;
    let start = Instant::now();
    for _ in 0..iterations {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        for &(slot, bit, threshold) in &bindings {
            samples[slot] = random & (1 << bit) != 0 && threshold < 15;
        }
        machine.step(&samples).unwrap();
        let word = output_indices.iter().fold(0u16, |w, &i| (w << 1) | u16::from(machine.outputs()[i] != 0));
        persistent_words.push(word);
        checksum = checksum.rotate_left(1) ^ std::hint::black_box(word);
    }
    let seconds = start.elapsed().as_secs_f64();
    let revision = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().unwrap();
    println!(
        "offline changing-input SettledMachine: revision={} (working tree may be dirty), O6 budget=8 stable_instant_inputs=true, iterations={iterations} seconds={seconds:.6}, evaluations/s={:.0}, six-tick equivalent/s={:.0}, bindings={} checksum={checksum:04x}",
        String::from_utf8_lossy(&revision.stdout).trim(),
        iterations as f64 / seconds,
        6.0 * iterations as f64 / seconds,
        bindings.len()
    );
    let mut fixed = crate::redpiler::program_graph::SettledEvaluation::new(&circuit).unwrap();
    let mut fixed_inputs = initial.clone();
    let fixed_bindings = circuit
        .program
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(slot, input)| {
            let LayerInputSource::SourceAbove { site, threshold, .. } = input.source else {
                return None;
            };
            let SiteSource::Component(node) = graph.sites[site.index()].source else {
                return None;
            };
            let pos = graph.nodes[node.index()].position?;
            inputs.iter().position(|p| *p == pos).map(|bit| (slot, bit, threshold))
        })
        .collect::<Vec<_>>();
    let fixed_feedback = circuit
        .program
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(slot, input)| {
            let LayerInputSource::State(StateTest::ResourceAt { resource, contact }) = input.source else {
                return None;
            };
            feedback
                .iter()
                .find(|(r, _, _)| *r == resource)
                .map(|(_, source, output)| (slot, *output, contact == *source))
        })
        .collect::<Vec<_>>();
    // Seed non-control sources, including buttons, from the same held native world.
    for (external, input) in machine.external_inputs().iter().zip(&samples) {
        if let Some(slot) = circuit.program.inputs.iter().position(|candidate| candidate.source == external.source) {
            fixed_inputs[slot] = *input;
        }
    }
    let oracle_vector = 0x4_1234_5678u64;
    for &(slot, bit, threshold) in &fixed_bindings {
        fixed_inputs[slot] = oracle_vector & (1 << bit) != 0 && threshold < 15;
    }
    for &(slot, _, polarity) in &fixed_feedback {
        fixed_inputs[slot] = polarity;
    }
    let mut oracle_converged = false;
    for _ in 0..128 {
        fixed.evaluate(&fixed_inputs).unwrap();
        let mut changed = false;
        for &(slot, output, polarity) in &fixed_feedback {
            let next = (fixed.outputs()[output] != 0) == polarity;
            changed |= fixed_inputs[slot] != next;
            fixed_inputs[slot] = next;
        }
        if !changed {
            oracle_converged = true;
            break;
        }
    }
    let fixed_oracle = output_indices.iter().fold(0u16, |w, &i| (w << 1) | u16::from(fixed.outputs()[i] != 0));
    println!("fixed-point oracle vector 0x412345678: result={fixed_oracle:04x} converged={oracle_converged}");
    assert!(oracle_converged);
    assert_eq!(fixed_oracle, 0x84f3, "user-confirmed arithmetic result");
    let mut random = 0x19a4110b5eedu64;
    let mut checksum = 0u16;
    let mut passes = 0u64;
    let mut converged = 0u64;
    let mut first_difference = None;
    let start = Instant::now();
    for index in 0..iterations {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        for &(slot, bit, threshold) in &fixed_bindings {
            fixed_inputs[slot] = random & (1 << bit) != 0 && threshold < 15;
        }
        for &(slot, _, polarity) in &fixed_feedback {
            fixed_inputs[slot] = polarity;
        }
        for _ in 0..128 {
            fixed.evaluate(&fixed_inputs).unwrap();
            passes += 1;
            let mut changed = false;
            for &(slot, output, polarity) in &fixed_feedback {
                let next = (fixed.outputs()[output] != 0) == polarity;
                changed |= fixed_inputs[slot] != next;
                fixed_inputs[slot] = next;
            }
            if !changed {
                converged += 1;
                break;
            }
        }
        let word = output_indices.iter().fold(0u16, |w, &i| (w << 1) | u16::from(fixed.outputs()[i] != 0));
        if first_difference.is_none() && word != persistent_words[index as usize] {
            first_difference = Some((index, random & ((1u64 << 35) - 1), persistent_words[index as usize], word));
        }
        checksum = checksum.rotate_left(1) ^ std::hint::black_box(word);
    }
    let seconds = start.elapsed().as_secs_f64();
    println!(
        "offline fixed-point: frames={iterations} seconds={seconds:.6} frames/s={:.0} six-tick equivalent/s={:.0} converged={converged} passes={passes} checksum={checksum:04x}",
        iterations as f64 / seconds,
        6.0 * iterations as f64 / seconds
    );
    println!("first feedback difference (index, raw manifest-order mask, persistent, fixed): {first_difference:016x?}");
    let (index, mask, persistent, fixed_result) = first_difference.expect("first feedback divergence");
    assert_eq!((index, mask), (2, 0x5_58e1_d715));
    // User confirmed all ones is invalid for this vector; the fixed result is not certified.
    assert_eq!(persistent, 0xffff);
    assert_ne!(fixed_result, 0xffff);
    {
        let mut replay = crate::redpiler::program_graph::SettledMachine::new(&circuit, &initial).unwrap();
        for external in &history {
            replay.step(external).unwrap();
        }
        let mut random = 0x19a4110b5eedu64;
        for _ in 0..=index {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            for &(slot, bit, threshold) in &bindings {
                samples[slot] = random & (1 << bit) != 0 && threshold < 15;
            }
            replay.step(&samples).unwrap();
        }
        let replay_word = output_indices.iter().fold(0u16, |w, &i| (w << 1) | u16::from(replay.outputs()[i] != 0));
        assert_eq!(random & ((1u64 << 35) - 1), mask);
        assert_eq!(replay_word, persistent, "fresh machine must reproduce first differing word");
        for &(slot, bit, threshold) in &fixed_bindings {
            fixed_inputs[slot] = random & (1 << bit) != 0 && threshold < 15;
        }
        for &(slot, _, polarity) in &fixed_feedback {
            fixed_inputs[slot] = polarity;
        }
        let mut stable = false;
        for _ in 0..128 {
            fixed.evaluate(&fixed_inputs).unwrap();
            let mut changed = false;
            for &(slot, output, polarity) in &fixed_feedback {
                let next = (fixed.outputs()[output] != 0) == polarity;
                changed |= fixed_inputs[slot] != next;
                fixed_inputs[slot] = next;
            }
            if !changed {
                stable = true;
                break;
            }
        }
        let replay_fixed = output_indices.iter().fold(0u16, |w, &i| (w << 1) | u16::from(fixed.outputs()[i] != 0));
        assert!(stable);
        assert_eq!(replay_fixed, fixed_result, "reset fixed point must reproduce first differing word");
        let mut direct = crate::redpiler::program_graph::SettledMachine::new(&circuit, &initial).unwrap();
        direct.step(&samples).unwrap();
        let direct_word = output_indices.iter().fold(0u16, |w, &i| (w << 1) | u16::from(direct.outputs()[i] != 0));
        println!("fresh direct vector without warmup: persistent={direct_word:04x} fixed={replay_fixed:04x}");
        println!("fresh replay confirmed first differing result: persistent={replay_word:04x} fixed={replay_fixed:04x}");
    }
    assert_eq!(converged, iterations, "fixed-point frames must converge");
    // This explores one transition per resource, not physical callback order or same-value events.
    let owners = feedback
        .iter()
        .map(|(resource, _, _)| {
            let owners = graph
                .plans
                .instant
                .boundaries
                .as_ref()
                .unwrap()
                .cells
                .iter()
                .flat_map(|cell| cell.resources.iter())
                .filter(|pin| pin.resource == *resource)
                .flat_map(|pin| pin.owners.iter().copied())
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(owners.len(), 1, "feedback resource must have one catalog actor");
            *owners.first().unwrap()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(feedback.len(), 15);
    assert_eq!(owners.len(), feedback.len(), "feedback resources must have distinct actors");
    for mask in [0x4_1234_5678u64, 0x5_58e1_d715u64] {
        let mut once_inputs = fixed_inputs.clone();
        for &(slot, bit, threshold) in &fixed_bindings {
            once_inputs[slot] = mask & (1 << bit) != 0 && threshold < 15;
        }
        for &(slot, _, polarity) in &fixed_feedback {
            once_inputs[slot] = polarity;
        }
        let mut evaluator = crate::redpiler::program_graph::SettledEvaluation::new(&circuit).unwrap();
        let mut fired = vec![false; fixed_feedback.len()];
        let mut conflict = false;
        for _ in 0..=fixed_feedback.len() {
            evaluator.evaluate(&once_inputs).unwrap();
            let mut changed = false;
            for (i, &(slot, output, polarity)) in fixed_feedback.iter().enumerate() {
                let next = (evaluator.outputs()[output] != 0) == polarity;
                if next != once_inputs[slot] {
                    if fired[i] {
                        conflict = true;
                    } else {
                        fired[i] = true;
                        once_inputs[slot] = next;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let word = output_indices
            .iter()
            .fold(0u16, |w, &i| (w << 1) | u16::from(evaluator.outputs()[i] != 0));
        println!(
            "one-transition-per-resource diagnostic: mask={mask:09x} output={word:04x} changed_resources={} repeat_requested={conflict}",
            fired.iter().filter(|x| **x).count()
        );
    }
    // User-provided arithmetic result for this raw input vector.
    assert_eq!(results[2].2, 0x84f3);
}
