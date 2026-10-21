//! O6 settled-DAG inspection and vector checks for the parsed 11-bit adder.
use super::*;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::{CompilerOptions, passes};
use crate::world::World;
use mchprs_blocks::blocks::{Block, LeverFace};
use mchprs_blocks::{BlockFace, BlockPos};
use std::collections::HashMap;
use std::path::Path;

fn prepare_adder() -> (serde_json::Value, crate::plot::PlotWorld, CompileGraph) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/adder_11bits.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let world = super::cpus::load_manifest(&manifest).unwrap();
    let monitor = TaskMonitor::default();
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(&world, world.get_corners(), vec![], &monitor, AnalysisLimits::for_budget(8)).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(6),
        stable_instant_inputs: true,
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
    (manifest, world, prepared.graph)
}

fn manifest_positions(manifest: &serde_json::Value, port: &str) -> Vec<BlockPos> {
    let origin = super::cpus::manifest_origin(manifest);
    manifest["ports"]["inputs"][port]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + super::cpus::manifest_position(position))
        .collect()
}

fn manifest_outputs(manifest: &serde_json::Value) -> Vec<BlockPos> {
    let origin = super::cpus::manifest_origin(manifest);
    manifest["ports"]["observations"]["sum_repeater"]
        .as_array()
        .unwrap()
        .iter()
        .map(|position| origin + super::cpus::manifest_position(position))
        .collect()
}

fn set_lever(world: &mut crate::plot::PlotWorld, position: BlockPos, powered: bool) {
    let Block::Lever { mut lever } = world.get_block(position) else {
        panic!("expected adder input lever at {position:?}")
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

fn power_value(graph: &CompileGraph, source: LayerInputSource, world: &crate::plot::PlotWorld) -> Result<bool, String> {
    match source {
        LayerInputSource::SourceAbove { site, port, threshold } => {
            let spec = graph.sites[site.index()];
            let (position, block) = match spec.source {
                SiteSource::Component(node) => {
                    let position = graph.nodes[node.index()].position.ok_or("source component has no position")?;
                    (position, world.get_block(position))
                }
                SiteSource::Contact(contact) => {
                    let position = graph.contacts[contact.index()].position;
                    (position, world.get_block(position))
                }
                SiteSource::Constant(_) => return Ok(spec.source_strength > threshold),
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
                PowerPort::Conducted { .. } => return Err("conducted source should have been expanded into wire equations".into()),
            };
            Ok(strength > threshold)
        }
        LayerInputSource::State(test) => state_value(graph, test, world),
        other => Err(format!("adder DAG contains unsupported boundary input {other:?}")),
    }
}

fn state_value(graph: &CompileGraph, test: StateTest, world: &crate::plot::PlotWorld) -> Result<bool, String> {
    match test {
        StateTest::PistonExtended(piston) => {
            let spec = graph.pistons[piston.index()];
            Ok(matches!(world.get_block(spec.position), Block::Piston { piston } if piston.extended))
        }
        StateTest::PistonHeadPresent(piston) => {
            let spec = graph.pistons[piston.index()];
            Ok(matches!(world.get_block(spec.position.offset(spec.facing)), Block::PistonHead { .. }))
        }
        StateTest::PistonAvailability { is, .. } => Ok(is == PistonAvailability::Ready),
        StateTest::ResourceAt { resource, contact } => {
            let position = graph.contacts[contact.index()].position;
            let block = Block::from_id(graph.profiles[graph.resources[resource.index()].material.index()].raw_state);
            Ok(world.get_block(position) == block)
        }
        StateTest::ResourceEnabled(resource) => {
            let block = Block::from_id(graph.profiles[graph.resources[resource.index()].material.index()].raw_state);
            Ok(graph.resources[resource.index()]
                .contacts
                .iter()
                .any(|contact| world.get_block(graph.contacts[contact.index()].position) == block))
        }
    }
}

fn evaluate_sum(graph: &CompileGraph, world: &crate::plot::PlotWorld, output_positions: &[BlockPos]) -> Result<u16, String> {
    let circuit = graph.plans.settled_circuit.as_ref().ok_or("O6 did not publish a settled circuit")?;
    let inputs = circuit
        .program
        .inputs
        .iter()
        .map(|input| power_value(graph, input.source, world))
        .collect::<Result<Vec<_>, _>>()?;
    let mut evaluation = SettledEvaluation::new(circuit)?;
    evaluation.evaluate(&inputs).map_err(str::to_owned)?;
    let mut sum = 0u16;
    for (bit, &position) in output_positions.iter().enumerate() {
        let node = graph
            .nodes
            .iter()
            .position(|node| node.position == Some(position))
            .ok_or_else(|| format!("missing output repeater node at {position:?}"))?;
        let target = SettledTarget::Power(PowerTarget::Input {
            node: NodeId::new(node as u32),
            port: InputPort::Main,
        });
        let index = circuit.outputs.iter().position(|output| output.target == target).ok_or_else(|| {
            format!(
                "missing settled output binding for bit {bit} at {position:?}; declines: {:?}",
                circuit.declines
            )
        })?;
        let powered = evaluation.outputs()[index] != 0;
        if !powered {
            sum |= 1 << bit;
        }
    }
    Ok(sum)
}

fn assert_repeater_is_a_state_leaf(mut graph: CompileGraph, port: InputPort, attenuation: u8) {
    let node = graph
        .nodes
        .iter()
        .position(|node| matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })))
        .map(|index| NodeId::new(index as u32))
        .expect("adder fixture has an intermediate repeater");
    let site = graph
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(node))
        .map(|index| SiteId::new(index as u32))
        .expect("repeater has an output site");
    let guard = graph
        .guards
        .iter()
        .position(|guard| matches!(guard, Guard::Always))
        .expect("always guard") as u32;
    graph.power_links.push(PowerLink {
        from: site,
        from_port: PowerPort::Output,
        to: PowerTarget::Input { node, port },
        face: BlockFace::Top,
        attenuation,
        guard: GuardId::new(guard),
    });
    let monitor = TaskMonitor::default();
    let mut equations = Equations::new(&graph, &monitor).unwrap();
    equations.inline_repeaters = true;
    equations.retain_classic = true;
    let expression = equations
        .source(LayerInputSource::SourceAbove {
            site,
            port: PowerPort::Output,
            threshold: 0,
        })
        .unwrap();
    let LayerExpr::Input(input) = equations.reduced.expression(expression).unwrap() else {
        panic!("locked or cyclic repeater must remain an explicit current-state leaf")
    };
    assert_eq!(
        equations.reduced.inputs[input.0 as usize].source,
        LayerInputSource::SourceAbove {
            site,
            port: PowerPort::Output,
            threshold: 0,
        }
    );
    assert!(equations.stateful_repeaters.contains(&node));
}

#[test]
fn adder_11bits_o6_settled_graph_reports_bindings_and_declines() {
    let (manifest, _, graph) = prepare_adder();
    let circuit = graph.plans.settled_circuit.as_ref().expect("O6 should publish a settled artifact");
    let mut input_kinds = HashMap::new();
    let mut input_sources = HashMap::new();
    let mut canonical_binary_inputs = 0;
    for input in circuit.program.inputs.iter() {
        *input_kinds.entry(format!("{:?}", input.source)).or_insert(0usize) += 1;
        if let LayerInputSource::SourceAbove { site, port, threshold } = input.source {
            if let SiteSource::Component(node) = graph.sites[site.index()].source {
                let spec = &graph.nodes[node.index()];
                *input_sources
                    .entry(format!("{:?} at {:?} via {port:?} threshold {threshold}", spec.kind, spec.position))
                    .or_insert(0usize) += 1;
                if matches!(
                    spec.kind,
                    CircuitNodeKind::Electrical(
                        NodeType::Lever
                            | NodeType::Button
                            | NodeType::PressurePlate
                            | NodeType::Repeater { .. }
                            | NodeType::Torch
                            | NodeType::Observer { .. }
                    ) | CircuitNodeKind::Observer { .. }
                        | CircuitNodeKind::BuiltinReset { .. }
                ) {
                    canonical_binary_inputs += 1;
                    assert_eq!(port, PowerPort::Output, "binary source must use its canonical direction");
                    assert_eq!(threshold, 0, "binary source must use its canonical positive threshold");
                }
            }
        }
    }
    println!(
        "adder_11bits O6: nodes={} inputs={} expressions={} outputs={} retained_effects={} declines={}",
        graph.nodes.len(),
        circuit.program.inputs.len(),
        circuit.program.expressions.len(),
        circuit.outputs.len(),
        circuit.retained_effects.len(),
        circuit.declines.len(),
    );
    println!("input boundaries: {input_kinds:#?}");
    println!("input source nodes: {input_sources:#?}");
    println!("canonical binary source inputs: {canonical_binary_inputs}");
    for decline in circuit.declines.iter() {
        println!("settled decline: {decline}");
    }
    for output in circuit.outputs.iter() {
        println!("settled output: {:?} {:?} roots={:?}", output.target, output.kind, output.roots);
    }
    assert!(circuit.validate().is_ok());
    assert_eq!(manifest["dimensions"], serde_json::json!([23, 7, 46]));
    assert_eq!(manifest_outputs(&manifest).len(), 11);

    let mut input_positions = manifest_positions(&manifest, "A");
    input_positions.extend(manifest_positions(&manifest, "B"));
    let trigger = super::cpus::manifest_origin(&manifest) + super::cpus::manifest_position(&manifest["ports"]["inputs"]["trigger"]);
    input_positions.push(trigger);
    for input in circuit.program.inputs.iter() {
        if let LayerInputSource::SourceAbove { site, .. } = input.source {
            let SiteSource::Component(node) = graph.sites[site.index()].source else {
                continue;
            };
            let position = graph.nodes[node.index()].position.expect("source component position");
            assert!(
                input_positions.contains(&position),
                "settled DAG still samples an internal computed source {:?} at {position:?}",
                graph.nodes[node.index()].kind
            );
        }
    }
    assert_eq!(
        circuit.program.inputs.len(),
        23,
        "expected only the 22 operand levers and trigger at the boundary"
    );
}

#[test]
fn adder_11bits_settled_dag_matches_native_carry_and_overflow_vectors() {
    let (manifest, _, graph) = prepare_adder();
    let a_positions = manifest_positions(&manifest, "A");
    let b_positions = manifest_positions(&manifest, "B");
    let output_positions = manifest_outputs(&manifest);
    assert_eq!(a_positions.len(), 11);
    assert_eq!(b_positions.len(), 11);
    assert_eq!(output_positions.len(), 11);

    // Include long carry chains and modulo-2048 overflow without enumerating the input space.
    for (a, b) in [(0u16, 0u16), (31, 1), (1023, 1), (1024, 1024), (2047, 1), (2047, 2047), (1365, 682)] {
        let mut native = super::cpus::load_manifest(&manifest).unwrap();
        for (bit, &position) in a_positions.iter().enumerate() {
            set_lever(&mut native, position, a & (1 << bit) == 0);
        }
        for (bit, &position) in b_positions.iter().enumerate() {
            set_lever(&mut native, position, b & (1 << bit) == 0);
        }
        let origin = super::cpus::manifest_origin(&manifest);
        let trigger = origin + super::cpus::manifest_position(&manifest["ports"]["inputs"]["trigger"]);
        set_lever(&mut native, trigger, true);
        for _ in 0..32 {
            native.tick_interpreted();
        }
        set_lever(&mut native, trigger, false);
        for _ in 0..24 {
            native.tick_interpreted();
        }

        let native_sum = output_positions.iter().enumerate().fold(0u16, |sum, (bit, &position)| {
            let Block::RedstoneRepeater { repeater } = native.get_block(position) else {
                panic!("sum output repeater disappeared at {position:?}")
            };
            if !repeater.powered { sum | (1 << bit) } else { sum }
        });
        let logical_sum =
            evaluate_sum(&graph, &native, &output_positions).unwrap_or_else(|error| panic!("settled evaluation failed for {a}+{b}: {error}"));
        let expected = (a + b) & 0x07ff;
        assert_eq!(native_sum, expected, "native vector {a}+{b}");
        assert_eq!(logical_sum, native_sum, "settled DAG vector {a}+{b}");
    }
}

#[test]
fn repeater_inlining_keeps_side_locked_and_feedback_nodes_as_state() {
    let (_, _, graph) = prepare_adder();
    assert_repeater_is_a_state_leaf(graph.clone(), InputPort::Side, 0);
    // A one-level attenuated self dependency revisits the same repeater with a
    // different SourceAbove threshold, so cycle detection must key by node ID.
    assert_repeater_is_a_state_leaf(graph, InputPort::Main, 1);
}
