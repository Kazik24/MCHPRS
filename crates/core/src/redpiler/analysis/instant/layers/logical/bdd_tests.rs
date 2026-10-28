use super::*;

fn lower_program_bdd(bdd: &mut BddManager, program: &ReducedLayerProgram, input_roots: &[u32]) -> Vec<u32> {
    let mut expression_roots = Vec::with_capacity(program.expressions.len());
    for expression in program.expressions.iter() {
        let root = match expression {
            LayerExpr::Constant(value) => u32::from(*value),
            LayerExpr::Input(input) => input_roots[input.0 as usize],
            LayerExpr::Not(child) => bdd.negate(expression_roots[child.0 as usize]).unwrap(),
            LayerExpr::All(children) => children
                .iter()
                .fold(1, |acc, child| bdd.and(acc, expression_roots[child.0 as usize]).unwrap()),
            LayerExpr::Any(children) => children
                .iter()
                .fold(0, |acc, child| bdd.or(acc, expression_roots[child.0 as usize]).unwrap()),
        };
        expression_roots.push(root);
    }
    expression_roots
}

fn unknown_input(
    bdd: &mut BddManager,
    unknowns: &mut rustc_hash::FxHashMap<LayerInputSource, u32>,
    next_unknown: &mut u16,
    source: LayerInputSource,
) -> u32 {
    if let Some(&root) = unknowns.get(&source) {
        return root;
    }
    let root = bdd.variable(*next_unknown).unwrap();
    *next_unknown = next_unknown.checked_add(1).expect("bounded counter inputs");
    unknowns.insert(source, root);
    root
}

#[test]
fn shared_boolean_operations_reduce_to_the_same_function() {
    let mut bdd = BddManager::default();
    let a = bdd.variable(0).unwrap();
    let b = bdd.variable(1).unwrap();
    let not_b = bdd.negate(b).unwrap();
    let ab = bdd.and(a, b).unwrap();
    let a_not_b = bdd.and(a, not_b).unwrap();
    assert_eq!(bdd.or(ab, a_not_b).unwrap(), a);
    assert_eq!(bdd.xor(a, a).unwrap(), 0);
    assert_eq!(bdd.xor(b, not_b).unwrap(), 1);
    let not_a = bdd.negate(a).unwrap();
    let not_ab = bdd.negate(ab).unwrap();
    assert_eq!(bdd.or(not_a, not_b).unwrap(), not_ab);
}

#[test]
fn sixteen_bit_carry_functions_share_a_small_decision_dag() {
    let mut bdd = BddManager::default();
    let mut carry = 1;
    for bit in 0..16 {
        let input = bdd.variable(bit).unwrap();
        let next = bdd.xor(input, carry).unwrap();
        assert_ne!(next, input);
        let zero = bdd.negate(input).unwrap();
        carry = bdd.and(carry, zero).unwrap();
    }
    assert!(bdd.nodes.len() < 1024, "carry chains must not expand into input cases");
}

#[test]
fn counter_fixture_storage_equations_have_a_symbolic_controlled_word_proof() {
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::{CompilerOptions, passes};
    use rustc_hash::{FxHashMap, FxHashSet};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let world = super::super::cpus::load_manifest(&manifest).unwrap();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, world.get_corners(), vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();

    let mut equations = Equations::new(&prepared.graph, &monitor).unwrap();
    let reads = equations.registers.values().copied().collect::<Vec<_>>();
    let mut registers = reads
        .into_iter()
        .map(|read| {
            let root = equations.powered(read.storage).unwrap();
            let dependencies = equation_inputs(&equations.reduced, root)
                .iter()
                .filter(|input| matches!(input, LayerInputSource::State(StateTest::ResourceAt { .. })))
                .count();
            (dependencies, read, root)
        })
        .collect::<Vec<_>>();
    registers.sort_unstable_by_key(|&(dependencies, _, _)| dependencies);

    assert_eq!(registers.len(), 16, "fixture should expose a 16-bit storage word");
    assert!(registers.iter().enumerate().all(|(bit, &(dependencies, _, _))| dependencies == bit + 1));
    let controls: FxHashSet<_> = registers
        .iter()
        .flat_map(|&(_, _, root)| equation_inputs(&equations.reduced, root))
        .filter(|input| !matches!(input, LayerInputSource::State(StateTest::ResourceAt { .. })))
        .collect();
    let [LayerInputSource::SourceAbove { site, .. }] = controls.into_iter().collect::<Vec<_>>()[..] else {
        panic!("counter equations should have exactly one Boolean control")
    };
    let SiteSource::Component(control) = prepared.graph.sites[site.index()].source else {
        panic!("counter control must be a component")
    };
    let bit_by_resource: FxHashMap<_, _> = registers.iter().enumerate().map(|(bit, (_, read, _))| (read.resource, bit)).collect();

    prove_word_transition(&equations, &registers, &bit_by_resource, control, &monitor).unwrap();
}

#[test]
#[ignore = "native counter clock and boundary mapping diagnostic"]
fn counter_fixture_native_clock_and_register_mapping() {
    use crate::plot::PlotWorld;
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::{CompilerOptions, passes};
    use crate::world::World;
    use mchprs_blocks::BlockFace;
    use mchprs_blocks::blocks::{Block, LeverFace};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let mut world = super::super::cpus::load_manifest(&manifest).unwrap();
    let origin = super::super::cpus::manifest_origin(&manifest);
    let trigger = origin + super::super::cpus::manifest_position(&manifest["ports"]["inputs"]["trigger"]);
    let Block::Lever { mut lever } = world.get_block(trigger) else {
        panic!("trigger is not a lever")
    };
    assert_eq!(lever.powered, false, "fixture starts idle");

    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, world.get_corners(), vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let graph = prepared.graph;
    let reads = graph.plans.bud_memory_reads.clone();
    let mut by_z = reads.clone();
    by_z.sort_unstable_by_key(|read| graph.pistons[read.storage.index()].position.z);
    let memory_positions: Vec<_> = manifest["ports"]["observations"]["memory"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + super::super::cpus::manifest_position(position))
        .collect();
    let repeater_positions: Vec<_> = manifest["ports"]["observations"]["repeater"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + super::super::cpus::manifest_position(position))
        .collect();
    let output_input_positions: Vec<_> = manifest["ports"]["observations"]["output_input"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + super::super::cpus::manifest_position(position))
        .collect();
    println!("counter native trigger={trigger:?} read_count={}", reads.len());
    for (bit, read) in by_z.iter().enumerate() {
        println!(
            "counter bit={bit} storage={:?} resource={:?} source={:?} manifest={:?}",
            graph.pistons[read.storage.index()].position,
            read.resource,
            graph.contacts[read.source.index()].position,
            memory_positions.get(bit),
        );
    }
    let mut equations = Equations::new(&graph, &monitor).unwrap();
    let read_roots: Vec<_> = by_z.iter().map(|read| equations.powered(read.storage).unwrap()).collect();
    let mut leaves = rustc_hash::FxHashSet::default();
    for root in read_roots {
        leaves.extend(equation_inputs(&equations.reduced, root));
    }
    for leaf in leaves {
        match leaf {
            LayerInputSource::SourceAbove { site, port, threshold } => {
                let source = graph.sites[site.index()].source;
                let node = match source {
                    SiteSource::Component(node) => Some(&graph.nodes[node.index()]),
                    _ => None,
                };
                println!("counter leaf source-above site={site:?} port={port:?} threshold={threshold} source={source:?} node={node:?}");
            }
            other => println!("counter leaf {other:?}"),
        }
    }

    lever.powered = true;
    world.set_block(trigger, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(&mut world, trigger);
    let support = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(&mut world, trigger.offset(support));
    let read_word = |world: &PlotWorld| -> Option<u16> {
        memory_positions
            .iter()
            .enumerate()
            .try_fold(0, |word, (bit, &position)| match world.get_block(position) {
                Block::Piston { piston } => Some(word | (u16::from(!piston.extended) << bit)),
                _ => None,
            })
    };
    let read_repeater_word = |world: &PlotWorld| -> Option<u16> {
        repeater_positions
            .iter()
            .enumerate()
            .try_fold(0, |word, (bit, &position)| match world.get_block(position) {
                Block::RedstoneRepeater { repeater } => Some(word | (u16::from(!repeater.powered) << bit)),
                _ => None,
            })
    };
    let mut previous = read_word(&world).expect("saved counter memory starts stationary");
    println!(
        "counter native tick=0 stable_word={previous:#06x} repeater_word={:?} first_output_input={:?}",
        read_repeater_word(&world),
        world.get_block(output_input_positions[0]),
    );
    for tick in 1..=144 {
        world.tick_interpreted();
        if tick <= 18 {
            println!(
                "counter outputs tick={tick} memory={:?} repeater_word={:?} first_output_input={:?}",
                read_word(&world),
                read_repeater_word(&world),
                world.get_block(output_input_positions[0]),
            );
        }
        let Some(word) = read_word(&world) else {
            println!("counter native tick={tick} memory=TRANSIENT");
            continue;
        };
        if word != previous {
            let reader_pos = graph.pistons[graph.plans.bud_memory_reads[0].reader.index()].position;
            let reader = world.get_block(reader_pos);
            println!("counter native tick={tick} word={word:#06x} previous={previous:#06x} reader_pos={reader_pos:?} reader={reader:?}");
            previous = word;
        }
    }
}

#[test]
fn settled_counter_machine_matches_native_stable_word_through_byte_carry() {
    use crate::plot::PlotWorld;
    use crate::redpiler::analysis::AnalysisLimits;
    use crate::redpiler::{CompilerOptions, passes};
    use crate::world::World;
    use mchprs_blocks::BlockFace;
    use mchprs_blocks::blocks::{Block, LeverFace};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let mut world = super::super::cpus::load_manifest(&manifest).unwrap();
    let origin = super::super::cpus::manifest_origin(&manifest);
    let trigger = origin + super::super::cpus::manifest_position(&manifest["ports"]["inputs"]["trigger"]);
    let Block::Lever { mut lever } = world.get_block(trigger) else {
        panic!("trigger is not a lever")
    };
    assert!(!lever.powered);

    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, world.get_corners(), vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(5),
        budget_multiplier: 8,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::default_graph_pipeline(&options),
        &monitor,
    )
    .unwrap();
    let graph = prepared.graph;
    let circuit = compile_settled(&graph, &monitor).unwrap();
    assert!(
        circuit.declines.is_empty(),
        "counter settled circuit must be complete: {:?}",
        circuit.declines
    );

    let mut reads = graph.plans.bud_memory_reads.to_vec();
    reads.sort_unstable_by_key(|read| graph.pistons[read.storage.index()].position.z);
    assert_eq!(reads.len(), 16);
    let positions: Vec<_> = manifest["ports"]["observations"]["memory"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + super::super::cpus::manifest_position(position))
        .collect();
    let repeater_positions: Vec<_> = manifest["ports"]["observations"]["repeater"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + super::super::cpus::manifest_position(position))
        .collect();
    let resource_bits: rustc_hash::FxHashMap<_, _> = reads.iter().enumerate().map(|(bit, read)| (read.resource, bit)).collect();
    assert_eq!(
        reads.iter().map(|read| graph.pistons[read.storage.index()].position).collect::<Vec<_>>(),
        positions,
        "the physical BUD reads map directly to the manifest's LSB-first memory ports"
    );
    let output_root_indices: Vec<_> = repeater_positions
        .iter()
        .map(|position| {
            let node = graph.nodes.iter().position(|node| node.position == Some(*position)).unwrap();
            circuit
                .outputs
                .iter()
                .position(|output| {
                    output.target
                        == SettledTarget::Power(PowerTarget::Input {
                            node: NodeId::new(node as u32),
                            port: InputPort::Main,
                        })
                })
                .unwrap_or_else(|| panic!("settled outputs lack Main input at {position:?}"))
        })
        .collect();
    assert_eq!(output_root_indices.len(), 16);

    let native_word = |world: &PlotWorld| -> Option<u16> {
        positions
            .iter()
            .enumerate()
            .try_fold(0, |word, (bit, &position)| match world.get_block(position) {
                Block::Piston { piston } => Some(word | (u16::from(!piston.extended) << bit)),
                _ => None,
            })
    };
    let sample_source = |source: LayerInputSource, world: &PlotWorld| -> bool {
        let LayerInputSource::SourceAbove { site, port, threshold } = source else {
            panic!("unexpected non-register counter boundary input: {source:?}")
        };
        let spec = graph.sites[site.index()];
        let (position, block) = match spec.source {
            SiteSource::Component(node) => {
                let position = graph.nodes[node.index()].position.unwrap();
                (position, world.get_block(position))
            }
            SiteSource::Contact(contact) => {
                let position = graph.contacts[contact.index()].position;
                (position, world.get_block(position))
            }
            SiteSource::Constant(_) => return spec.source_strength > threshold,
        };
        let strength = match port {
            PowerPort::Output | PowerPort::Stored => crate::redstone::source_strength(block, world, position),
            PowerPort::Weak(face) => {
                if crate::redstone::power::emits_weak_power(block, world, position, face, true) {
                    crate::redstone::source_strength(block, world, position)
                } else {
                    0
                }
            }
            PowerPort::Strong(face) => {
                if crate::redstone::power::emits_strong_power(block, world, position, face, true) {
                    crate::redstone::source_strength(block, world, position)
                } else {
                    0
                }
            }
            PowerPort::Conducted { .. } => panic!("counter external input must be a source boundary"),
        };
        strength > threshold
    };

    let mut register_leaves = rustc_hash::FxHashSet::default();
    let mut pending_roots: Vec<_> = circuit
        .outputs
        .iter()
        .filter(|output| matches!(output.target, SettledTarget::Register(_)))
        .flat_map(|output| output.roots.iter().copied())
        .collect();
    let mut visited = rustc_hash::FxHashSet::default();
    while let Some(root) = pending_roots.pop() {
        if !visited.insert(root) {
            continue;
        }
        match &circuit.program.expressions[root.0 as usize] {
            LayerExpr::Input(input) => {
                register_leaves.insert(circuit.program.inputs[input.0 as usize].source);
            }
            LayerExpr::Not(child) => pending_roots.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending_roots.extend(children.iter().copied()),
            LayerExpr::Constant(_) => {}
        }
    }
    let controls: Vec<_> = register_leaves
        .into_iter()
        .filter(|source| match source {
            LayerInputSource::State(StateTest::ResourceAt { resource, .. }) => !resource_bits.contains_key(resource),
            _ => true,
        })
        .collect();
    assert_eq!(controls.len(), 1, "register outputs should share one control leaf: {controls:?}");
    let control_source = controls[0];
    println!("counter production register control={control_source:?}");

    // Prove the production artifact itself, without expanding assignments.
    // Torch HIGH is the reset/idle level; LOW decrements the stored far word.
    let mut bdd = BddManager::default();
    let torch_high = bdd.variable(0).unwrap();
    let bits: Vec<_> = (0usize..16).map(|bit| bdd.variable(bit as u16 + 1).unwrap()).collect();
    let mut unknowns = rustc_hash::FxHashMap::default();
    let mut next_unknown = 17u16;
    let mut input_roots = Vec::with_capacity(circuit.program.inputs.len());
    for input in circuit.program.inputs.iter() {
        let root = match input.source {
            LayerInputSource::SourceAbove { .. } if input.source == control_source => torch_high,
            LayerInputSource::State(StateTest::ResourceAt { resource, contact }) if resource_bits.contains_key(&resource) => {
                let bit = resource_bits[&resource];
                let read = reads[bit];
                if contact == read.source {
                    bits[bit]
                } else {
                    let piston = graph.pistons[read.storage.index()];
                    let other = match piston.lane {
                        PistonLane::SingleResource { far } if read.source == piston.near => far,
                        PistonLane::SingleResource { far } if read.source == far => piston.near,
                        _ => panic!("counter register input is outside its single-resource lane"),
                    };
                    if contact == other {
                        bdd.negate(bits[bit]).unwrap()
                    } else {
                        unknown_input(&mut bdd, &mut unknowns, &mut next_unknown, input.source)
                    }
                }
            }
            source => unknown_input(&mut bdd, &mut unknowns, &mut next_unknown, source),
        };
        input_roots.push(root);
    }
    let expression_roots = lower_program_bdd(&mut bdd, &circuit.program, &input_roots);
    let mut register_roots: Vec<_> = circuit
        .outputs
        .iter()
        .filter_map(|output| match output.target {
            SettledTarget::Register(read) => Some((resource_bits[&read.resource], output.roots[0])),
            SettledTarget::Power(_) | SettledTarget::ResetExcludedPower { .. } => None,
            SettledTarget::ResourceState { .. } | SettledTarget::ComponentOutput(_) => None,
        })
        .collect();
    register_roots.sort_unstable_by_key(|(bit, _)| *bit);
    assert_eq!(register_roots.len(), 16);
    let mut lower_bits_zero = 1;
    for (bit, (_, output_root)) in register_roots.iter().enumerate() {
        let decremented = if bit == 0 {
            bdd.negate(bits[bit]).unwrap()
        } else {
            bdd.xor(bits[bit], lower_bits_zero).unwrap()
        };
        let expected = bdd.or(torch_high, decremented).unwrap();
        assert_eq!(
            expression_roots[output_root.0 as usize], expected,
            "production register bit {bit} is exactly active-low controlled decrement with idle reset"
        );
        let bit_is_zero = bdd.negate(bits[bit]).unwrap();
        lower_bits_zero = bdd.and(lower_bits_zero, bit_is_zero).unwrap();
    }
    let machine_word = |values: &[bool]| -> u16 {
        circuit.program.inputs.iter().enumerate().fold(0, |word, (slot, input)| {
            if let LayerInputSource::State(StateTest::ResourceAt { resource, .. }) = input.source
                && let Some(&bit) = resource_bits.get(&resource)
                && values[slot]
            {
                word | (1 << bit)
            } else {
                word
            }
        })
    };
    let machine_output_word = |values: &[u8]| -> u16 {
        output_root_indices
            .iter()
            .enumerate()
            // These roots are power delivered to each output repeater. A powered repeater
            // represents a zero in the fixture's active-low public output word.
            .fold(0, |word, (bit, &index)| word | (u16::from(values[index] == 0) << bit))
    };
    let native_repeater_word = |world: &PlotWorld| -> Option<u16> {
        repeater_positions
            .iter()
            .enumerate()
            .try_fold(0, |word, (bit, &position)| match world.get_block(position) {
                Block::RedstoneRepeater { repeater } => Some(word | (u16::from(!repeater.powered) << bit)),
                _ => None,
            })
    };

    let first_word = native_word(&world).expect("saved counter begins at a stationary boundary");
    let mut initial_inputs = vec![false; circuit.program.inputs.len()];
    for (slot, input) in circuit.program.inputs.iter().enumerate() {
        match input.source {
            LayerInputSource::State(StateTest::ResourceAt { resource, .. }) if resource_bits.contains_key(&resource) => {
                let bit = resource_bits[&resource];
                initial_inputs[slot] = first_word & (1 << bit) == 0;
            }
            LayerInputSource::SourceAbove { .. } => initial_inputs[slot] = sample_source(input.source, &world),
            other => panic!("counter has an unbound initial input {other:?}"),
        }
    }
    let mut machine = crate::redpiler::program_graph::SettledMachine::new(&circuit, &initial_inputs).unwrap();
    assert!(machine.external_inputs().iter().any(|input| input.source == control_source));
    assert_eq!(
        !machine_word(machine.input_values()),
        first_word,
        "the machine starts from the imported count"
    );
    for _ in 0..24 {
        world.tick_interpreted();
    }
    assert_eq!(native_word(&world), Some(first_word), "native trigger OFF holds the initial stable word");
    lever.powered = true;
    world.set_block(trigger, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(&mut world, trigger);
    let support = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(&mut world, trigger.offset(support));

    let mut last_native = first_word;
    let mut native_edges = 0;
    let mut output_frame_proven = false;
    let mut last_valid_computation_samples = initial_inputs
        .iter()
        .enumerate()
        .filter_map(|(slot, input)| (!matches!(circuit.program.inputs[slot].source, LayerInputSource::State(StateTest::ResourceAt { resource, .. }) if resource_bits.contains_key(&resource))).then_some(*input))
        .collect::<Vec<_>>();
    for tick in 1..=1_560 {
        world.tick_interpreted();
        let samples: Vec<_> = machine
            .external_inputs()
            .iter()
            .map(|input| sample_source(input.source, &world))
            .collect();
        if tick % 6 == 5 && tick >= 11 {
            // Observer callbacks can pulse during the physical transition. The settled
            // datapath consumes the last valid clock-frame sample, not that transient pulse.
            // Phase 5 is the stable publication cut; phase 4 still has transient outputs.
            machine.observe(&last_valid_computation_samples).unwrap();
            let logical_output = machine_output_word(machine.outputs());
            let native_output = native_repeater_word(&world).expect("native output repeaters are present");
            for bit in 0..16 {
                assert_eq!(
                    logical_output & (1 << bit),
                    native_output & (1 << bit),
                    "settled output bit {bit} at {:?} differs at tick {tick}: root strength {}, native repeater {:?}",
                    repeater_positions[bit],
                    machine.outputs()[output_root_indices[bit]],
                    world.get_block(repeater_positions[bit]),
                );
            }
            if tick <= 23 {
                println!(
                    "counter publication tick={tick} logical_power={logical_output:#06x} native_repeater={:?}",
                    native_repeater_word(&world)
                );
            }
        }
        if tick % 6 == 0 {
            machine.step(&samples).unwrap();
            last_valid_computation_samples = samples
                .iter()
                .enumerate()
                .filter_map(|(external, &value)| {
                    let source = machine.external_inputs()[external].source;
                    (!matches!(source, LayerInputSource::State(StateTest::ResourceAt { resource, .. }) if resource_bits.contains_key(&resource)))
                        .then_some(value)
                })
                .collect();
            if !output_frame_proven {
                let mut frame_bdd = BddManager::default();
                let bits: Vec<_> = (0..16).map(|bit| frame_bdd.variable(bit as u16).unwrap()).collect();
                let mut sampled_external = rustc_hash::FxHashMap::default();
                for (input, &value) in machine.external_inputs().iter().zip(&samples) {
                    sampled_external.insert(input.source, value);
                }
                let mut unknowns = rustc_hash::FxHashMap::default();
                let mut next_unknown = 16u16;
                let frame_input_roots: Vec<_> = circuit
                    .program
                    .inputs
                    .iter()
                    .map(|input| match input.source {
                        LayerInputSource::State(StateTest::ResourceAt { resource, contact }) if resource_bits.contains_key(&resource) => {
                            let bit = resource_bits[&resource];
                            let read = reads[bit];
                            let piston = graph.pistons[read.storage.index()];
                            if contact == read.source {
                                bits[bit]
                            } else {
                                let other = match piston.lane {
                                    PistonLane::SingleResource { far } if read.source == piston.near => far,
                                    PistonLane::SingleResource { far } if read.source == far => piston.near,
                                    _ => panic!("counter register input is outside its storage lane"),
                                };
                                if contact == other {
                                    frame_bdd.negate(bits[bit]).unwrap()
                                } else {
                                    unknown_input(&mut frame_bdd, &mut unknowns, &mut next_unknown, input.source)
                                }
                            }
                        }
                        LayerInputSource::SourceAbove { .. } => u32::from(*sampled_external.get(&input.source).expect("sampled frame source")),
                        source => unknown_input(&mut frame_bdd, &mut unknowns, &mut next_unknown, source),
                    })
                    .collect();
                let frame_expression_roots = lower_program_bdd(&mut frame_bdd, &circuit.program, &frame_input_roots);
                for bit in 0..16 {
                    let output = &circuit.outputs[output_root_indices[bit]];
                    assert_eq!(
                        frame_expression_roots[output.roots[0].0 as usize], bits[bit],
                        "valid phase-0 frame: final repeater input bit {bit} is the stored q bit for every word"
                    );
                }
                output_frame_proven = true;
            }
        }
        let Some(word) = native_word(&world) else {
            continue; // The native pistons are moving; compare only stationary storage cuts.
        };
        if word == last_native {
            continue;
        }
        assert_eq!(word, last_native.wrapping_add(1), "native storage advances by one at tick {tick}");
        assert_eq!(tick % 6, 0, "the native stable storage cadence remains six ticks");
        assert_eq!(
            !machine_word(machine.input_values()),
            word,
            "logical machine at stable native tick {tick}"
        );
        last_native = word;
        native_edges += 1;
    }
    assert!(
        native_edges >= 250,
        "trajectory should cross the 8-bit carry boundary, observed {native_edges} increments"
    );
    assert_eq!(last_native, 260, "native and logical runs cover 0x00ff -> 0x0100");
    assert!(output_frame_proven, "the stable phase-0 input frame must prove the output bank");

    // Exercise representative borrow/carry and full-width wrap seeds without
    // enumerating the word space; native trajectory above validates the real
    // circuit's 0x00ff -> 0x0100 boundary.
    for seed in [0x0000u16, 0x0001, 0x00ff, 0x0100, 0x7fff, 0xffff] {
        let mut seed_inputs = initial_inputs.clone();
        for (slot, input) in circuit.program.inputs.iter().enumerate() {
            match input.source {
                LayerInputSource::State(StateTest::ResourceAt { resource, .. }) if resource_bits.contains_key(&resource) => {
                    let bit = resource_bits[&resource];
                    // The stored far-contact word is the complement of
                    // the externally observed count word.
                    seed_inputs[slot] = seed & (1 << bit) == 0;
                }
                source if source == control_source => seed_inputs[slot] = false,
                LayerInputSource::SourceAbove { .. } => seed_inputs[slot] = sample_source(input.source, &world),
                other => panic!("counter has an unbound seed input {other:?}"),
            }
        }
        let mut seeded = crate::redpiler::program_graph::SettledMachine::new(&circuit, &seed_inputs).unwrap();
        let active: Vec<_> = seeded
            .external_inputs()
            .iter()
            .map(|input| {
                if input.source == control_source {
                    false
                } else {
                    sample_source(input.source, &world)
                }
            })
            .collect();
        seeded.step(&active).unwrap();
        let mut stored = 0u16;
        for (slot, input) in circuit.program.inputs.iter().enumerate() {
            if let LayerInputSource::State(StateTest::ResourceAt { resource, .. }) = input.source
                && let Some(&bit) = resource_bits.get(&resource)
                && seeded.input_values()[slot]
            {
                stored |= 1 << bit;
            }
        }
        assert_eq!(!stored, seed.wrapping_add(1), "active-low seeded count={seed:#06x}");
    }
}

#[test]
fn native_counter_off_freezes_and_reenable_resumes_at_each_clock_phase() {
    use crate::plot::PlotWorld;
    use crate::world::World;
    use mchprs_blocks::BlockFace;
    use mchprs_blocks::blocks::{Block, LeverFace};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let origin = super::super::cpus::manifest_origin(&manifest);
    let trigger = origin + super::super::cpus::manifest_position(&manifest["ports"]["inputs"]["trigger"]);
    let positions: Vec<_> = manifest["ports"]["observations"]["memory"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + super::super::cpus::manifest_position(position))
        .collect();
    let stable_word = |world: &PlotWorld| -> Option<u16> {
        positions
            .iter()
            .enumerate()
            .try_fold(0, |word, (bit, &position)| match world.get_block(position) {
                Block::Piston { piston } => Some(word | (u16::from(!piston.extended) << bit)),
                _ => None,
            })
    };

    for phase in 0..6 {
        let mut world = super::super::cpus::load_manifest(&manifest).unwrap();
        let Block::Lever { mut lever } = world.get_block(trigger) else {
            panic!("trigger is not a lever")
        };
        lever.powered = true;
        world.set_block(trigger, Block::Lever { lever });
        crate::redstone::update_surrounding_blocks(&mut world, trigger);
        let support = match lever.face {
            LeverFace::Floor => BlockFace::Bottom,
            LeverFace::Ceiling => BlockFace::Top,
            LeverFace::Wall => lever.facing.opposite().block_face(),
        };
        crate::redstone::update_surrounding_blocks(&mut world, trigger.offset(support));
        let mut before_off = stable_word(&world);
        for _ in 0..(18 + phase) {
            world.tick_interpreted();
            if let Some(word) = stable_word(&world) {
                before_off = Some(word);
            }
        }
        let Block::Lever { mut lever } = world.get_block(trigger) else {
            unreachable!()
        };
        lever.powered = false;
        world.set_block(trigger, Block::Lever { lever });
        crate::redstone::update_surrounding_blocks(&mut world, trigger);
        crate::redstone::update_surrounding_blocks(&mut world, trigger.offset(support));
        let mut last_stable = before_off;
        let mut post_off_edges = Vec::new();
        for _ in 0..24 {
            world.tick_interpreted();
            if let Some(word) = stable_word(&world)
                && last_stable != Some(word)
            {
                post_off_edges.push(word);
                last_stable = Some(word);
            }
        }
        let final_word = stable_word(&world).expect("the memory word should settle after control OFF");
        println!("counter OFF phase={phase} before={before_off:?} post_off_edges={post_off_edges:?} final={final_word}");
        let expected_final = if phase == 0 { 3 } else { 4 };
        let expected_edges = if phase == 0 { vec![] } else { vec![4] };
        assert_eq!(before_off, Some(3), "the trigger runs three completed steps before cut phase {phase}");
        assert_eq!(
            post_off_edges, expected_edges,
            "only an already-started update can finish after OFF at phase {phase}"
        );
        assert_eq!(final_word, expected_final, "OFF freezes the phase-dependent in-flight result");
        for _ in 0..6 {
            world.tick_interpreted();
        }
        assert_eq!(
            stable_word(&world),
            Some(final_word),
            "the count remains frozen after OFF at phase {phase}"
        );
        let Block::Lever { mut lever } = world.get_block(trigger) else {
            unreachable!()
        };
        lever.powered = true;
        world.set_block(trigger, Block::Lever { lever });
        crate::redstone::update_surrounding_blocks(&mut world, trigger);
        crate::redstone::update_surrounding_blocks(&mut world, trigger.offset(support));
        for _ in 0..36 {
            world.tick_interpreted();
        }
        assert_eq!(
            stable_word(&world),
            Some(final_word.wrapping_add(6)),
            "re-enabling resumes six stable count steps after phase {phase}"
        );
    }
}
