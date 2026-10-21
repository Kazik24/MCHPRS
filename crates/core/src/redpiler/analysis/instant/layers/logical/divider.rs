//! O6 extraction and point-in-time differential checks for the fixed FPU divider.
use super::*;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::{CompilerOptions, passes};
use crate::world::World;
use mchprs_blocks::blocks::{Block, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use std::collections::BTreeMap;
use std::path::Path;

fn prepare_divider() -> (serde_json::Value, crate::plot::PlotWorld, CompileGraph) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/piston-research/fixtures/fpu_divider.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let origin = BlockPos::new(40, 30, 40);
    manifest["origin"] = serde_json::json!([origin.x, origin.y, origin.z]);
    let world = cpus::load_manifest(&manifest).unwrap();
    let dimensions = &manifest["dimensions"];
    let bounds = (
        origin,
        origin
            + BlockPos::new(
                dimensions[0].as_i64().unwrap() as i32 - 1,
                dimensions[1].as_i64().unwrap() as i32 - 1,
                dimensions[2].as_i64().unwrap() as i32 - 1,
            ),
    );
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(6),
        budget_multiplier: 8,
        stable_instant_inputs: true,
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
    (manifest, world, prepared.graph)
}

#[test]
#[ignore = "expensive O6 divider extraction and native differential diagnostic"]
fn report_and_differential_check_divider_settled_equations() {
    let (manifest, mut world, graph) = prepare_divider();
    let circuit = graph.plans.settled_circuit.as_ref().expect("O6 should publish a settled artifact");
    let origin = cpus::manifest_origin(&manifest);
    let outputs: Vec<_> = manifest["observations"]["output_msb_first"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + cpus::manifest_position(position))
        .collect();
    let inputs_a: Vec<_> = manifest["observations"]["input_a_msb_first"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + cpus::manifest_position(position))
        .collect();
    let inputs_b: Vec<_> = manifest["observations"]["input_b_msb_first"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + cpus::manifest_position(position))
        .collect();
    let trigger = origin + cpus::manifest_position(&manifest["protocol"]["trigger"]);
    let mut input_kinds = BTreeMap::<String, usize>::new();
    for input in circuit.program.inputs.iter() {
        *input_kinds.entry(input_kind(&graph, input.source)).or_default() += 1;
    }
    let mut output_kinds = BTreeMap::<String, usize>::new();
    for output in circuit.outputs.iter() {
        *output_kinds.entry(format!("{:?}", output.target)).or_default() += 1;
    }
    let output_nodes: Vec<_> = outputs
        .iter()
        .map(|position| {
            let (index, node) = graph
                .nodes
                .iter()
                .enumerate()
                .find(|(_, node)| node.position == Some(*position))
                .expect("manifest output repeater must have a prepared node");
            assert!(matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })));
            NodeId::new(index as u32)
        })
        .collect();
    let compare_outputs: Vec<_> = circuit
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| match output.target {
            SettledTarget::Power(PowerTarget::Input { node, port: InputPort::Main })
                if output_nodes.contains(&node) && !root_samples_node(&graph, circuit, output.roots[0], node) =>
            {
                Some((index, node))
            }
            _ => None,
        })
        .collect();
    println!(
        "divider O6 settled: nodes={} pistons={} resources={} cells={} bud_registers={} inputs={} expressions={} outputs={} retained_effects={} declines={}",
        graph.nodes.len(),
        graph.pistons.len(),
        graph.resources.len(),
        graph.plans.instant.boundaries.as_ref().map_or(0, |catalog| catalog.cells.len()),
        graph.plans.bud_memory_reads.len(),
        circuit.program.inputs.len(),
        circuit.program.expressions.len(),
        circuit.outputs.len(),
        circuit.retained_effects.len(),
        circuit.declines.len(),
    );
    println!("divider logical input kinds: {input_kinds:#?}");
    println!("divider logical output targets: {output_kinds:#?}");
    println!(
        "output repeater targets present in artifact: {}/{}",
        compare_outputs.len(),
        output_nodes.len()
    );
    for (index, node) in compare_outputs.iter().map(|(index, node)| (*index, *node)) {
        println!(
            "divider output target {:?} {:?} at {:?}",
            circuit.outputs[index].target,
            graph.nodes[node.index()].kind,
            graph.nodes[node.index()].position
        );
    }
    for decline in circuit.declines.iter() {
        println!("divider settled decline: {decline}");
    }

    assert_eq!(inputs_a.len(), 10);
    assert_eq!(inputs_b.len(), 10);
    assert_eq!(outputs.len(), 10);
    assert!(matches!(world.get_block(trigger), Block::Lever { lever } if lever.powered));
    println!("native differential comparison count: {} repeater Main targets", compare_outputs.len());

    // Match the benchmark's raw bit convention (manifest array index is bit index).
    // Vectors differ from the saved operands; no arithmetic result is assumed here.
    let vectors = [(0x001u16, 0x2aau16), (0x155, 0x0a0), (0x2aa, 0x155)];
    let mut evaluation = SettledEvaluation::new(circuit).unwrap();
    let mut checked_phases = 0usize;
    let mut skipped_phases = 0usize;
    let mut checked_outputs = 0usize;
    for (a, b) in vectors {
        set_bank(&mut world, &inputs_a, a);
        set_bank(&mut world, &inputs_b, b);
        advance(&mut world, 64);
        record_comparison(
            assert_artifact_matches_repeater_inputs(&graph, circuit, &mut evaluation, &world, &compare_outputs),
            &mut checked_phases,
            &mut skipped_phases,
            &mut checked_outputs,
        );

        set_lever(&mut world, trigger, false);
        advance(&mut world, 128);
        record_comparison(
            assert_artifact_matches_repeater_inputs(&graph, circuit, &mut evaluation, &world, &compare_outputs),
            &mut checked_phases,
            &mut skipped_phases,
            &mut checked_outputs,
        );

        set_lever(&mut world, trigger, true);
        advance(&mut world, 128);
        record_comparison(
            assert_artifact_matches_repeater_inputs(&graph, circuit, &mut evaluation, &world, &compare_outputs),
            &mut checked_phases,
            &mut skipped_phases,
            &mut checked_outputs,
        );
    }
    println!(
        "divider native differential coverage: checked_phases={checked_phases} skipped_phases={skipped_phases} checked_outputs={checked_outputs}"
    );
    assert!(checked_outputs > 0, "divider differential must compare at least one output");
    assert_eq!(skipped_phases, 0, "divider differential must sample every protocol phase");
}

fn input_kind(graph: &CompileGraph, source: LayerInputSource) -> String {
    match source {
        LayerInputSource::SourceAbove { site, port, threshold } => {
            let kind = match graph.sites[site.index()].source {
                SiteSource::Component(node) => format!("{:?}@{:?}", graph.nodes[node.index()].kind, graph.nodes[node.index()].position),
                SiteSource::Contact(contact) => format!("contact:{contact:?}"),
                SiteSource::Constant(profile) => format!("constant:{profile:?}"),
            };
            format!("source:{kind}:{port:?}:>{threshold}")
        }
        LayerInputSource::State(state) => format!("state:{state:?}"),
        other => format!("boundary:{other:?}"),
    }
}

fn assert_artifact_matches_repeater_inputs(
    graph: &CompileGraph,
    circuit: &SettledCircuit,
    evaluation: &mut SettledEvaluation,
    world: &crate::plot::PlotWorld,
    outputs: &[(usize, NodeId)],
) -> Option<usize> {
    let mut inputs = Vec::with_capacity(circuit.program.inputs.len());
    for input in circuit.program.inputs.iter() {
        let Some(value) = sample_input(graph, input.source, world) else {
            println!("divider differential skipped: no native adapter for {:?}", input.source);
            return None;
        };
        inputs.push(value);
    }
    evaluation.evaluate(&inputs).unwrap();
    for &(index, node) in outputs {
        let position = graph.nodes[node.index()].position.expect("output repeater has a position");
        let Block::RedstoneRepeater { repeater } = world.get_block(position) else {
            panic!("output repeater disappeared at {position:?}");
        };
        let front = position.offset(repeater.facing.block_face());
        let front_block = world.get_block(front);
        let mut strength = crate::redstone::get_redstone_power(front_block, world, front, repeater.facing.block_face());
        if strength == 0 {
            if let Block::RedstoneWire { wire } = front_block {
                strength = wire.power;
            }
        }
        assert_eq!(
            evaluation.outputs()[index] > 0,
            strength > 0,
            "settled logical Main input differs from native power at {position:?}"
        );
    }
    Some(outputs.len())
}

fn record_comparison(checked: Option<usize>, checked_phases: &mut usize, skipped_phases: &mut usize, checked_outputs: &mut usize) {
    match checked {
        Some(count) => {
            *checked_phases += 1;
            *checked_outputs += count;
        }
        None => *skipped_phases += 1,
    }
}

fn root_samples_node(graph: &CompileGraph, circuit: &SettledCircuit, root: LayerExprId, node: NodeId) -> bool {
    let mut pending = vec![root];
    let mut visited = FxHashSet::default();
    while let Some(expression) = pending.pop() {
        if !visited.insert(expression) {
            continue;
        }
        match &circuit.program.expressions[expression.0 as usize] {
            LayerExpr::Input(input) => {
                if let LayerInputSource::SourceAbove { site, .. } = circuit.program.inputs[input.0 as usize].source {
                    if graph.sites[site.index()].source == SiteSource::Component(node) {
                        return true;
                    }
                }
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
            LayerExpr::Constant(_) => {}
        }
    }
    false
}

fn sample_input(graph: &CompileGraph, source: LayerInputSource, world: &crate::plot::PlotWorld) -> Option<bool> {
    match source {
        LayerInputSource::SourceAbove { site, port, threshold } => {
            let (block, position) = match graph.sites[site.index()].source {
                SiteSource::Component(node) => {
                    let position = graph.nodes[node.index()].position?;
                    (world.get_block(position), position)
                }
                SiteSource::Contact(contact) => {
                    let position = graph.contacts[contact.index()].position;
                    (world.get_block(position), position)
                }
                SiteSource::Constant(profile) => {
                    let block = Block::from_id(graph.profiles[profile.index()].raw_state);
                    let position = graph.sites[site.index()].position;
                    return Some(power_emits(block, world, position, port) && graph.sites[site.index()].source_strength > threshold);
                }
            };
            Some(power_emits(block, world, position, port) && crate::redstone::source_strength(block, world, position) > threshold)
        }
        LayerInputSource::State(StateTest::ResourceAt { resource, contact }) => {
            let position = graph.contacts[contact.index()].position;
            let material = Block::from_id(graph.profiles[graph.resources[resource.index()].material.index()].raw_state);
            Some(world.get_block(position) == material)
        }
        LayerInputSource::State(StateTest::ResourceEnabled(resource)) => {
            let material = Block::from_id(graph.profiles[graph.resources[resource.index()].material.index()].raw_state);
            Some(
                graph.resources[resource.index()]
                    .contacts
                    .iter()
                    .any(|contact| world.get_block(graph.contacts[contact.index()].position) == material),
            )
        }
        LayerInputSource::State(StateTest::PistonExtended(piston)) => {
            let spec = graph.pistons[piston.index()];
            Some(matches!(world.get_block(spec.position), Block::Piston { piston } if piston.extended))
        }
        LayerInputSource::State(StateTest::PistonHeadPresent(piston)) => {
            let spec = graph.pistons[piston.index()];
            Some(matches!(world.get_block(spec.position.offset(spec.facing)), Block::PistonHead { .. }))
        }
        LayerInputSource::State(StateTest::PistonAvailability { piston, is }) => {
            let spec = graph.pistons[piston.index()];
            let head = spec.position.offset(spec.facing);
            let availability = match world.get_block(spec.position) {
                Block::MovingPiston { .. } => {
                    if matches!(world.get_block(head), Block::PistonHead { .. }) {
                        crate::redpiler::program_graph::PistonAvailability::BasePendingHeadPresent
                    } else {
                        crate::redpiler::program_graph::PistonAvailability::BasePending
                    }
                }
                _ if matches!(world.get_block(head), Block::MovingPiston { .. }) => crate::redpiler::program_graph::PistonAvailability::HeadPending,
                _ => crate::redpiler::program_graph::PistonAvailability::Ready,
            };
            // PublishingHeadRemoval is a graph-internal callback cut, not a native
            // world boundary state; stable samples therefore match no such guard.
            Some(availability == is)
        }
        _ => None,
    }
}

fn power_emits(block: Block, world: &crate::plot::PlotWorld, position: BlockPos, port: PowerPort) -> bool {
    match port {
        PowerPort::Output | PowerPort::Stored => true,
        PowerPort::Weak(face) => crate::redstone::power::emits_weak_power(block, world, position, face, true),
        PowerPort::Strong(face) => crate::redstone::power::emits_strong_power(block, world, position, face, true),
        PowerPort::Conducted { .. } => false,
    }
}

fn set_bank(world: &mut crate::plot::PlotWorld, bank: &[BlockPos], raw: u16) {
    for (bit, &position) in bank.iter().enumerate() {
        set_lever(world, position, raw & (1 << bit) != 0);
    }
}

fn set_lever(world: &mut crate::plot::PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("expected divider input lever at {position:?}");
    };
    if lever.powered == powered {
        return;
    }
    lever.powered = powered;
    world.set_block(position, Block::Lever { lever });
    crate::redstone::update_surrounding_blocks(world, position);
    let support = match lever.face {
        LeverFace::Floor => BlockFace::Bottom,
        LeverFace::Ceiling => BlockFace::Top,
        LeverFace::Wall => lever.facing.opposite().block_face(),
    };
    crate::redstone::update_surrounding_blocks(world, position.offset(support));
}

fn advance(world: &mut crate::plot::PlotWorld, ticks: usize) {
    for _ in 0..ticks {
        world.tick_interpreted();
    }
}
