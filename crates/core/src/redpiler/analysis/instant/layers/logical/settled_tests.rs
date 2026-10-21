use super::*;
use crate::plot::PlotWorld;
use crate::redpiler::analysis::AnalysisLimits;
use crate::redpiler::{CompilerOptions, passes};
use crate::world::World;
use crate::world::storage::Chunk;
use mchprs_blocks::blocks::{Block, ComparatorMode, RedstoneComparator, RedstoneObserver, RedstoneRepeater, RedstoneWire};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};
use std::path::Path;
use std::time::Instant;

pub(super) fn prepared_o6(world: &PlotWorld, bounds: (BlockPos, BlockPos), monitor: &TaskMonitor) -> CompileGraph {
    monitor.set_budget_multiplier(8);
    let mut prepared = passes::rich::prepare(world, bounds, vec![], monitor, AnalysisLimits::for_budget(8)).unwrap();
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
        monitor,
    )
    .unwrap();
    prepared.graph
}

fn tiny_world() -> PlotWorld {
    PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default())
}

fn counter_control_inputs(graph: &CompileGraph, circuit: &SettledCircuit, control_powered: bool) -> (Vec<bool>, Vec<usize>) {
    let mut expression_seen = vec![false; circuit.program.expressions.len()];
    let mut input_slots = FxHashSet::default();
    let mut pending = circuit
        .outputs
        .iter()
        .filter(|output| matches!(output.target, SettledTarget::Register(_)))
        .flat_map(|output| output.roots.iter().copied())
        .collect::<Vec<_>>();
    while let Some(root) = pending.pop() {
        let index = root.0 as usize;
        if expression_seen[index] {
            continue;
        }
        expression_seen[index] = true;
        match &circuit.program.expressions[index] {
            LayerExpr::Constant(_) => {}
            LayerExpr::Input(input) => {
                input_slots.insert(input.0 as usize);
            }
            LayerExpr::Not(child) => pending.push(*child),
            LayerExpr::All(children) | LayerExpr::Any(children) => pending.extend(children.iter().copied()),
        }
    }
    let control_sources = input_slots
        .iter()
        .filter_map(|&slot| match circuit.program.inputs[slot].source {
            LayerInputSource::SourceAbove { site, .. } => match graph.sites[site.index()].source {
                SiteSource::Component(node) => Some((site, node)),
                _ => None,
            },
            _ => None,
        })
        .collect::<FxHashSet<_>>();
    assert_eq!(control_sources.len(), 1, "counter register roots have one source-derived active control");
    let (control_site, control_node) = *control_sources.iter().next().unwrap();
    let node = &graph.nodes[control_node.index()];
    if control_powered {
        println!(
            "counter logical register control: site={control_site:?} position={:?} node={control_node:?} kind={:?} state={:?}",
            graph.sites[control_site.index()].position,
            node.kind,
            node.state,
        );
    }
    let mut register_slots = circuit
        .outputs
        .iter()
        .filter_map(|output| match output.target {
            SettledTarget::Register(read) => Some(read),
            SettledTarget::Power(_) => None,
            SettledTarget::ResourceState { .. } => None,
        })
        .collect::<Vec<_>>();
    register_slots.sort_unstable_by_key(|read| graph.pistons[read.storage.index()].position.z);
    let register_slots: Vec<usize> = register_slots
        .iter()
        .map(|read| {
            circuit
                .program
                .inputs
                .iter()
                .position(|input| {
                    matches!(input.source, LayerInputSource::State(StateTest::ResourceAt { resource, contact })
                        if resource == read.resource && contact == read.source)
                })
                .expect("every register output has its old-state input")
        })
        .collect();
    let register_slot_set: FxHashSet<_> = register_slots.iter().copied().collect();
    let external: Vec<bool> = circuit
        .program
        .inputs
        .iter()
        .enumerate()
        .filter(|(slot, _)| !register_slot_set.contains(slot))
        .map(|(_, input)| match input.source {
            LayerInputSource::SourceAbove { site, threshold, .. } => {
                let source = &graph.sites[site.index()];
                match source.source {
                    SiteSource::Component(node) if node == control_node => control_powered,
                    SiteSource::Component(node) if matches!(&graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lever)) => {
                        !control_powered
                    }
                    _ => source.source_strength > threshold,
                }
            }
            _ => false,
        })
        .collect();
    (external, register_slots)
}

fn counter_word(inputs: &[bool], register_slots: &[usize]) -> u32 {
    register_slots
        .iter()
        .enumerate()
        .fold(0, |word, (bit, &slot)| word | (u32::from(inputs[slot]) << bit))
}

fn counter_sample_inputs(graph: &CompileGraph, circuit: &SettledCircuit, word: u16, control_powered: bool) -> Vec<bool> {
    let (external, register_slots) = counter_control_inputs(graph, circuit, control_powered);
    let register_set: FxHashSet<_> = register_slots.iter().copied().collect();
    let mut inputs = vec![false; circuit.program.inputs.len()];
    for (bit, &slot) in register_slots.iter().enumerate() {
        inputs[slot] = word & (1 << bit) != 0;
    }
    let mut external = external.into_iter();
    for (slot, value) in inputs.iter_mut().enumerate() {
        if !register_set.contains(&slot) {
            *value = external.next().expect("one sampled value per non-register input");
        }
    }
    assert!(external.next().is_none(), "all sampled external inputs were consumed");
    inputs
}

fn remap_inputs_by_source(from: &SettledCircuit, values: &[bool], to: &SettledCircuit) -> Vec<bool> {
    to.program
        .inputs
        .iter()
        .map(|input| {
            let slot = from
                .program
                .inputs
                .iter()
                .position(|candidate| candidate.source == input.source)
                .unwrap_or_else(|| panic!("compact input is absent from full artifact: {:?}", input.source));
            values[slot]
        })
        .collect()
}

fn time_logical_steps(machine: &mut SettledMachine, external: &[bool], steps: u64) -> (std::time::Duration, u64) {
    let start = Instant::now();
    let mut operations = 0;
    for _ in 0..steps {
        operations += machine.step(external).unwrap() as u64;
    }
    (start.elapsed(), operations)
}

#[test]
fn settled_outputs_keep_comparator_strength_as_fifteen_thresholds() {
    let mut world = tiny_world();
    let source = BlockPos::new(4, 2, 4);
    let wire = source + BlockPos::new(1, 0, 0);
    let comparator = source + BlockPos::new(2, 0, 0);
    let repeater = source + BlockPos::new(1, 0, 1);
    for position in [source, wire, comparator, repeater] {
        world.set_block(position + BlockPos::new(0, -1, 0), Block::Stone {});
    }
    // A classic comparator is a physically valid 0..15 analog source. Feed its
    // boundary strength through the raw threshold sampler below; a lever can
    // only supply 0 or 15 and would not exercise analog preservation.
    world.set_block(
        source,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
        },
    );
    world.set_block(
        wire,
        Block::RedstoneWire {
            wire: RedstoneWire::default(),
        },
    );
    world.set_block(
        comparator,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
        },
    );
    world.set_block(
        repeater,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: BlockDirection::North,
                ..Default::default()
            },
        },
    );

    let monitor = TaskMonitor::default();
    let graph = prepared_o6(&world, (source + BlockPos::new(-2, -1, -2), source + BlockPos::new(5, 3, 5)), &monitor);
    let source_node = graph.nodes.iter().position(|node| node.position == Some(source)).unwrap();
    let comparator_node = graph.nodes.iter().position(|node| node.position == Some(comparator)).unwrap();
    let repeater_node = graph.nodes.iter().position(|node| node.position == Some(repeater)).unwrap();
    let target = PowerTarget::Input {
        node: NodeId::new(comparator_node as u32),
        port: InputPort::Main,
    };
    let boolean_target = PowerTarget::Input {
        node: NodeId::new(repeater_node as u32),
        port: InputPort::Main,
    };
    let circuit = compile_settled(&graph, &monitor).unwrap();
    let output_index = circuit
        .outputs
        .iter()
        .position(|output| output.target == SettledTarget::Power(target))
        .unwrap();
    let boolean_index = circuit
        .outputs
        .iter()
        .position(|output| output.target == SettledTarget::Power(boolean_target))
        .unwrap();
    let output = &circuit.outputs[output_index];
    assert_eq!(output.kind, SettledValueKind::Strength);
    assert_eq!(output.roots.len(), 15);
    assert_eq!(circuit.outputs[boolean_index].kind, SettledValueKind::Bool);
    assert_eq!(circuit.outputs[boolean_index].roots.len(), 1);

    let mut evaluation = SettledEvaluation::new(&circuit).unwrap();
    let mut values = Vec::with_capacity(16);
    for strength in 0..=15 {
        let inputs = circuit
            .program
            .inputs
            .iter()
            .map(|input| match input.source {
                LayerInputSource::SourceAbove { site, threshold, .. }
                    if graph.sites[site.index()].source == SiteSource::Component(NodeId::new(source_node as u32)) =>
                {
                    strength > threshold
                }
                _ => false,
            })
            .collect::<Vec<_>>();
        evaluation.evaluate(&inputs).unwrap();
        values.push(evaluation.outputs()[output_index]);
    }
    assert_eq!(
        values,
        (0..=15).collect::<Vec<_>>(),
        "a comparator source strength survives the dust line without quantizing"
    );
    assert_eq!(
        evaluation.outputs()[boolean_index],
        1,
        "the repeater boundary exposes a Boolean powered value"
    );

    let expected = graph.plans.settled_circuit.as_ref().expect("O6 pipeline publishes the settled artifact");
    let (program, _) = graph
        .clone()
        .lower_with_monitor(&monitor)
        .expect("the validated artifact lowers with the graph");
    assert_eq!(
        program.settled_circuit(),
        Some(expected),
        "lowering retains the source-derived settled artifact"
    );

    let mut tampered = graph.clone();
    let artifact = tampered.plans.settled_circuit.as_mut().expect("settled artifact");
    artifact.outputs[output_index].roots[0] = LayerExprId(u32::MAX);
    assert!(
        tampered.lower_with_monitor(&monitor).is_err(),
        "lowering rejects a tampered settled expression root"
    );
    let mut wrong_value = graph.clone();
    let artifact = wrong_value.plans.settled_circuit.as_mut().unwrap();
    artifact.outputs[output_index].roots[0] = artifact.outputs[output_index].roots[1];
    assert!(
        wrong_value.lower_with_monitor(&monitor).is_err(),
        "a well-formed but wrong output must fail source validation"
    );
}

#[test]
fn settled_boundary_keeps_external_observers_but_drops_internal_reset_callbacks() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let mut world = cpus::load_manifest(&manifest).unwrap();
    let monitor = TaskMonitor::default();
    let initial = prepared_o6(&world, world.get_corners(), &monitor);
    let watched = initial
        .plans
        .instant
        .boundaries
        .as_ref()
        .unwrap()
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter())
        .filter_map(|use_| initial.nodes[initial.pistons[use_.actor.index()].node.index()].position)
        .find_map(|target| {
            [
                (BlockFace::East, BlockFacing::East),
                (BlockFace::West, BlockFacing::West),
                (BlockFace::North, BlockFacing::North),
                (BlockFace::South, BlockFacing::South),
                (BlockFace::Top, BlockFacing::Up),
                (BlockFace::Bottom, BlockFacing::Down),
            ]
            .into_iter()
            .map(|(facing, block_facing)| (target, facing, block_facing))
            .find(|(target, facing, _)| world.get_block(target.offset(facing.opposite())) == Block::Air {})
        })
        .expect("recognized piston has a free adjacent position for an outside observer");
    let observer_pos = watched.0.offset(watched.1.opposite());
    world.set_block(
        observer_pos,
        Block::Observer {
            observer: RedstoneObserver {
                facing: watched.2,
                powered: false,
            },
        },
    );
    let graph = prepared_o6(&world, world.get_corners(), &monitor);
    let circuit = compile_settled(&graph, &monitor).unwrap();
    let observer_node = graph
        .nodes
        .iter()
        .position(|node| node.position == Some(observer_pos))
        .map(|index| NodeId::new(index as u32))
        .unwrap();
    let observer_site = graph
        .sites
        .iter()
        .position(|site| site.source == SiteSource::Component(observer_node))
        .map(|index| SiteId::new(index as u32))
        .unwrap();
    let observer_source = LayerInputSource::SourceAbove {
        site: observer_site,
        port: PowerPort::Output,
        threshold: 0,
    };
    let mut ordinary = Equations::new(&graph, &monitor).unwrap();
    ordinary.retain_classic = true;
    // Exercise an observer without an existing private-reset classification.
    ordinary.resets.remove(&observer_node);
    let ordinary_root = ordinary.source_value(observer_source).unwrap();
    assert!(matches!(ordinary.reduced.expression(ordinary_root).unwrap(), LayerExpr::Input(_)));

    let mut computation = Equations::new(&graph, &monitor).unwrap();
    computation.retain_classic = true;
    computation.resets.remove(&observer_node);
    super::settled::assume_computation_frame_observers_off(&mut computation);
    let computation_root = computation.source_value(observer_source).unwrap();
    assert_eq!(computation.reduced.expression(computation_root).unwrap(), &LayerExpr::Constant(false));

    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let mut owned: FxHashSet<_> = catalog.corridors.iter().flat_map(|corridor| corridor.wires.iter().copied()).collect();
    owned.extend(
        catalog
            .cells
            .iter()
            .flat_map(|cell| cell.uses.iter().map(|use_| graph.pistons[use_.actor.index()].node)),
    );
    owned.extend(catalog.cells.iter().flat_map(|cell| {
        cell.uses.iter().flat_map(|use_| {
            let piston = graph.pistons[use_.actor.index()];
            [piston.base, piston.near, piston.far].map(|contact| graph.contacts[contact.index()].node)
        })
    }));
    owned.extend(graph.plans.bud_memory_reads.iter().map(|read| graph.pistons[read.storage.index()].node));
    let retained: FxHashSet<_> = circuit.retained_effects.iter().map(|port| port.kind).collect();
    let register_outputs = circuit
        .outputs
        .iter()
        .filter(|output| matches!(output.target, SettledTarget::Register(_)))
        .count();
    println!(
        "settled counter extraction: nodes={} cells={} inputs={} expressions={} outputs={} registers={} retained_effects={} [guard_state={}, guard_site={}, contact_alternative={}, captured_wire={}, observer_watch={}, wire_relation={}, power={}, other={}] declines={}",
        graph.nodes.len(),
        catalog.cells.len(),
        circuit.program.inputs.len(),
        circuit.program.expressions.len(),
        circuit.outputs.len(),
        register_outputs,
        circuit.retained_effects.len(),
        circuit
            .retained_effects
            .iter()
            .filter(|port| matches!(port.kind, BoundaryPortKind::GuardState { .. }))
            .count(),
        circuit
            .retained_effects
            .iter()
            .filter(|port| matches!(port.kind, BoundaryPortKind::GuardSite { .. }))
            .count(),
        circuit
            .retained_effects
            .iter()
            .filter(|port| matches!(port.kind, BoundaryPortKind::ContactAlternative { .. }))
            .count(),
        circuit
            .retained_effects
            .iter()
            .filter(|port| matches!(port.kind, BoundaryPortKind::CapturedWire { .. }))
            .count(),
        circuit
            .retained_effects
            .iter()
            .filter(|port| matches!(port.kind, BoundaryPortKind::ObserverWatch { .. }))
            .count(),
        circuit
            .retained_effects
            .iter()
            .filter(|port| matches!(port.kind, BoundaryPortKind::WireRelation { .. }))
            .count(),
        circuit
            .retained_effects
            .iter()
            .filter(|port| matches!(port.kind, BoundaryPortKind::Power { .. }))
            .count(),
        circuit
            .retained_effects
            .iter()
            .filter(|port| !matches!(
                port.kind,
                BoundaryPortKind::GuardState { .. }
                    | BoundaryPortKind::GuardSite { .. }
                    | BoundaryPortKind::ContactAlternative { .. }
                    | BoundaryPortKind::CapturedWire { .. }
                    | BoundaryPortKind::ObserverWatch { .. }
                    | BoundaryPortKind::WireRelation { .. }
                    | BoundaryPortKind::Power { .. }
            ))
            .count(),
        circuit.declines.len(),
    );
    assert_eq!(register_outputs, 16, "all sixteen counter register roots are admitted");
    let reset_nodes: FxHashSet<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter().filter_map(|use_| use_.reset.observer()))
        .collect();
    assert!(
        circuit.program.inputs.iter().all(|input| match input.source {
            LayerInputSource::SourceAbove { site, .. } => match graph.sites[site.index()].source {
                SiteSource::Component(node) => !reset_nodes.contains(&node),
                _ => true,
            },
            _ => true,
        }),
        "private reset pulses must not become logical data inputs"
    );

    let outside_observers = catalog
        .ports
        .iter()
        .filter_map(|port| match port.kind {
            BoundaryPortKind::ObserverWatch { observer, .. }
                if port.direction == BoundaryDirection::Output
                    && catalog
                        .cells
                        .iter()
                        .all(|cell| cell.uses.iter().all(|use_| graph.pistons[use_.actor.index()].node != observer)) =>
            {
                Some(port.kind)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        !outside_observers.is_empty(),
        "fixture should expose an observer watch outside the recognized cells"
    );
    assert!(
        outside_observers.iter().any(|port| retained.contains(port)),
        "outside observer watches remain explicit effects"
    );

    let internal_callbacks = catalog
        .ports
        .iter()
        .filter(|port| {
            if port.direction != BoundaryDirection::Output {
                return false;
            }
            let BoundaryPortKind::Callback { occurrence } = port.kind else {
                return false;
            };
            let delivery = &graph.routes[occurrence.route.index()].deliveries[occurrence.ordinal as usize];
            match delivery.receiver {
                Some(CallbackReceiver::Component(node)) => catalog
                    .cells
                    .iter()
                    .any(|cell| cell.uses.iter().any(|use_| graph.pistons[use_.actor.index()].node == node)),
                Some(CallbackReceiver::PistonBase(piston) | CallbackReceiver::PistonHead(piston)) => {
                    catalog.cells.iter().any(|cell| cell.uses.iter().any(|use_| use_.actor == piston))
                }
                _ => false,
            }
        })
        .collect::<Vec<_>>();
    assert!(
        !internal_callbacks.is_empty(),
        "fixture should contain callbacks among recognized cell actors"
    );
    assert!(
        internal_callbacks.iter().all(|port| !retained.contains(&port.kind)),
        "internal reset callbacks are represented by settled outputs, not retained effects"
    );

    let internal_guard_states = catalog
        .ports
        .iter()
        .filter(|port| {
            if port.direction != BoundaryDirection::Output {
                return false;
            }
            let BoundaryPortKind::GuardState { test, .. } = port.kind else {
                return false;
            };
            match test {
                StateTest::PistonExtended(piston) | StateTest::PistonHeadPresent(piston) | StateTest::PistonAvailability { piston, .. } => {
                    owned.contains(&graph.pistons[piston.index()].node)
                }
                StateTest::ResourceAt { resource, .. } | StateTest::ResourceEnabled(resource) => graph.resources[resource.index()]
                    .contacts
                    .iter()
                    .all(|contact| owned.contains(&graph.contacts[contact.index()].node)),
            }
        })
        .collect::<Vec<_>>();
    assert!(
        !internal_guard_states.is_empty(),
        "fixture should expose guard-state support shared only by owned cells"
    );
    assert!(
        internal_guard_states.iter().all(|port| !retained.contains(&port.kind)),
        "state guards consumed only by owned cells are internal logical dependencies"
    );

    // This part checks relocation of the cold diagnostic effect frontier. O6's
    // separate device artifact intentionally slices it down to stable value roots.
    let mut diagnostic_graph = graph.clone();
    diagnostic_graph.plans.settled_circuit = Some(circuit.clone());
    let (program, _) = diagnostic_graph.lower_with_monitor(&monitor).unwrap();
    let lowered = program.settled_circuit().unwrap();
    assert_eq!(lowered.retained_effects.len(), circuit.retained_effects.len());
    let mut power_effects = 0;
    for (before, after) in circuit.retained_effects.iter().zip(lowered.retained_effects.iter()) {
        if let BoundaryPortKind::Power { link: before_link } = before.kind {
            let BoundaryPortKind::Power { link: after_link } = after.kind else {
                panic!("lowering changed the boundary effect kind")
            };
            assert_eq!(
                graph.power_links[before_link as usize],
                program.power_links()[after_link as usize],
                "sorting power links must preserve each retained effect's target"
            );
            power_effects += 1;
        }
    }
    assert!(power_effects > 0, "fixture exercises retained power-link effects");
}

#[test]
fn compact_counter_public_boundary_matches_full_artifact_for_requested_outputs_and_registers() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let world = cpus::load_manifest(&manifest).unwrap();
    let origin = cpus::manifest_origin(&manifest);
    let monitor = TaskMonitor::default();
    let graph = prepared_o6(&world, world.get_corners(), &monitor);
    let full = compile_settled(&graph, &monitor).unwrap();

    let output_positions = manifest["ports"]["observations"]["repeater"]
        .as_array()
        .expect("counter output repeaters");
    assert_eq!(output_positions.len(), 16);
    let output_targets = output_positions
        .iter()
        .map(|position| {
            let position = origin + cpus::manifest_position(position);
            let (index, node) = graph
                .nodes
                .iter()
                .enumerate()
                .find(|(_, node)| node.position == Some(position))
                .unwrap_or_else(|| panic!("manifest output repeater missing at {position:?}"));
            assert!(matches!(node.kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. })));
            PowerTarget::Input {
                node: NodeId::new(index as u32),
                port: InputPort::Main,
            }
        })
        .collect::<Vec<_>>();
    let register_reads = graph.plans.bud_memory_reads.clone();
    assert_eq!(register_reads.len(), 16);
    let compact = compile_public_boundary(&graph, &output_targets, &register_reads, &monitor).unwrap();

    assert!(compact.retained_effects.is_empty());
    assert!(compact.declines.is_empty());
    assert_eq!(
        compact.outputs.len(),
        32,
        "only sixteen requested outputs plus sixteen register roots are published"
    );
    assert!(
        compact.outputs[..16]
            .iter()
            .all(|output| matches!(output.target, SettledTarget::Power(_)))
    );
    assert!(
        compact.outputs[16..]
            .iter()
            .all(|output| matches!(output.target, SettledTarget::Register(_)))
    );
    for target in output_targets.iter().map(|&target| SettledTarget::Power(target)) {
        assert!(
            full.outputs.iter().any(|output| output.target == target),
            "full artifact contains {target:?}"
        );
        assert!(
            compact.outputs.iter().any(|output| output.target == target),
            "compact artifact contains {target:?}"
        );
    }
    for &read in &register_reads {
        let target = SettledTarget::Register(read);
        assert!(full.outputs.iter().any(|output| output.target == target));
        assert!(compact.outputs.iter().any(|output| output.target == target));
    }

    let mut full_eval = SettledEvaluation::new(&full).unwrap();
    let mut compact_eval = SettledEvaluation::new(&compact).unwrap();
    for word in [0x0000u16, 0x0001, 0x00ff, 0x0100, 0x01ff, 0x7fff, 0x8000, 0xffff, 0xa55a] {
        for control in [false, true] {
            let full_inputs = counter_sample_inputs(&graph, &full, word, control);
            let compact_inputs = remap_inputs_by_source(&full, &full_inputs, &compact);
            full_eval.evaluate(&full_inputs).unwrap();
            compact_eval.evaluate(&compact_inputs).unwrap();
            for output in &compact.outputs {
                let full_index = full.outputs.iter().position(|candidate| candidate.target == output.target).unwrap();
                let compact_index = compact.outputs.iter().position(|candidate| candidate.target == output.target).unwrap();
                assert_eq!(
                    compact_eval.outputs()[compact_index],
                    full_eval.outputs()[full_index],
                    "target={:?} register_word={word:#06x} control={control}",
                    output.target
                );
            }
        }
    }
}

#[test]
#[ignore = "release logical-kernel comparison; reports logical evaluations, not game TPS"]
fn settled_counter_heap_vs_bitmap_release_microbenchmark() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let world = cpus::load_manifest(&manifest).unwrap();
    let monitor = TaskMonitor::default();
    let graph = prepared_o6(&world, world.get_corners(), &monitor);
    let circuit = compile_settled(&graph, &monitor).unwrap();
    assert!(circuit.declines.is_empty(), "the benchmark needs complete register equations");
    let (active_external, register_slots) = counter_control_inputs(&graph, &circuit, true);
    assert_eq!(register_slots.len(), 16, "counter fixture is a 16-bit register word");
    // Far-contact state bits are the complement of the native counter word:
    // all resources start retracted (q=1) while the displayed count is 0.
    let mut initial_inputs = vec![false; circuit.program.inputs.len()];
    for &slot in &register_slots {
        initial_inputs[slot] = true;
    }

    let inactive_external = counter_control_inputs(&graph, &circuit, false).0;
    let mut active_probe = SettledMachine::new(&circuit, &initial_inputs).unwrap();
    active_probe.step(&active_external).unwrap();
    let active_count = (!counter_word(active_probe.input_values(), &register_slots)) & 0xffff;
    let mut inactive_probe = SettledMachine::new(&circuit, &initial_inputs).unwrap();
    inactive_probe.step(&inactive_external).unwrap();
    let inactive_count = (!counter_word(inactive_probe.input_values(), &register_slots)) & 0xffff;
    let (external, first_count) = if active_count == 1 && inactive_count == 0 {
        (active_external, active_count)
    } else if inactive_count == 1 && active_count == 0 {
        (inactive_external, inactive_count)
    } else {
        panic!("neither control level produces the proved +1 transition: active={active_count}, inactive={inactive_count}");
    };
    assert_eq!(first_count, 1);
    assert_eq!(
        external,
        [false, false, true],
        "native stable count cuts sample Torch off, Observer off, and trigger Lever on"
    );

    let mut bitmap = SettledMachine::new(&circuit, &initial_inputs).unwrap();
    let mut heap = SettledMachine::new(&circuit, &initial_inputs).unwrap();
    heap.set_heap_reference_for_test(true);
    for step in 0..256u32 {
        bitmap.step(&external).unwrap();
        heap.step(&external).unwrap();
        let expected_count = step + 1;
        let q = counter_word(bitmap.input_values(), &register_slots);
        assert_eq!((!q) & 0xffff, expected_count, "far-contact bits complement the counter word");
        assert_eq!(bitmap.input_values(), heap.input_values());
        assert_eq!(bitmap.outputs(), heap.outputs());
    }

    let steps = 65_536u64;
    let exprs = circuit.program.expressions.len();
    let mut heap_ns = Vec::with_capacity(5);
    let mut bitmap_ns = Vec::with_capacity(5);
    let mut heap_operations = 0;
    let mut bitmap_operations = 0;
    for pair in 0..5 {
        let mut heap = SettledMachine::new(&circuit, &initial_inputs).unwrap();
        heap.set_heap_reference_for_test(true);
        let mut bitmap = SettledMachine::new(&circuit, &initial_inputs).unwrap();
        // Exclude the initial full-graph evaluation; both timed runs start at count 1.
        heap.step(&external).unwrap();
        bitmap.step(&external).unwrap();
        assert_eq!(heap.input_values(), bitmap.input_values());
        let (heap_elapsed, heap_ops, bitmap_elapsed, bitmap_ops) = if pair % 2 == 0 {
            let (elapsed, ops) = time_logical_steps(&mut heap, &external, steps);
            let (bitmap_elapsed, bitmap_ops) = time_logical_steps(&mut bitmap, &external, steps);
            (elapsed, ops, bitmap_elapsed, bitmap_ops)
        } else {
            let (elapsed, ops) = time_logical_steps(&mut bitmap, &external, steps);
            let (heap_elapsed, heap_ops) = time_logical_steps(&mut heap, &external, steps);
            (heap_elapsed, heap_ops, elapsed, ops)
        };
        assert_eq!(heap.input_values(), bitmap.input_values());
        assert_eq!(heap.outputs(), bitmap.outputs());
        assert_eq!(
            (!counter_word(bitmap.input_values(), &register_slots)) & 0xffff,
            1,
            "the displayed 16-bit count returns to 1 after a complete 65,536-step cycle"
        );
        heap_operations += heap_ops;
        bitmap_operations += bitmap_ops;
        let heap_step_ns = heap_elapsed.as_secs_f64() * 1e9 / steps as f64;
        let bitmap_step_ns = bitmap_elapsed.as_secs_f64() * 1e9 / steps as f64;
        heap_ns.push(heap_step_ns);
        bitmap_ns.push(bitmap_step_ns);
        println!(
            "settled counter logical kernel pair={} exprs={exprs} steps={steps} order={} heap_ns/step={heap_step_ns:.1} bitmap_ns/step={bitmap_step_ns:.1}",
            pair + 1,
            if pair % 2 == 0 { "heap-bitmap" } else { "bitmap-heap" },
        );
    }
    heap_ns.sort_by(f64::total_cmp);
    bitmap_ns.sort_by(f64::total_cmp);
    println!(
        "settled counter logical kernel median: exprs={exprs} steps/pair={steps} pairs=5 heap_ops/step={:.1} bitmap_ops/step={:.1} heap_median_ns/step={:.1} bitmap_median_ns/step={:.1} heap_logical_steps/s={:.0} bitmap_logical_steps/s={:.0}",
        heap_operations as f64 / (steps * 5) as f64,
        bitmap_operations as f64 / (steps * 5) as f64,
        heap_ns[2],
        bitmap_ns[2],
        1e9 / heap_ns[2],
        1e9 / bitmap_ns[2],
    );
}

#[test]
fn external_capture_keeps_contact_alternatives_only_while_the_wire_is_outside() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/instant-pistons-io/fixtures/counter_basic.json");
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let world = cpus::load_manifest(&manifest).unwrap();
    let monitor = TaskMonitor::default();
    let graph = prepared_o6(&world, world.get_corners(), &monitor);
    let (wire, occurrence, neighbor, contact) = graph
        .wires
        .iter()
        .find_map(|wire| {
            let route = wire.turbo_route.or(wire.shape_route)?;
            if graph.routes[route.index()].deliveries.is_empty() {
                return None;
            }
            wire.canonical_neighbors.iter().find_map(|site| match graph.sites[site.index()].source {
                SiteSource::Contact(contact) if !graph.contacts[contact.index()].variants.is_empty() => {
                    Some((wire.node, RouteOccurrence { route, ordinal: 0 }, *site, contact))
                }
                _ => None,
            })
        })
        .expect("fixture has a wire route adjacent to a changing contact");
    let capture = BoundaryPort {
        owner: BoundaryOwner::Cell(0),
        direction: BoundaryDirection::Output,
        kind: BoundaryPortKind::CapturedWire { wire, occurrence, neighbor },
    };
    let alternative = BoundaryPortKind::ContactAlternative { contact, ordinal: 0 };
    let mut catalog = BoundaryCatalog::default();
    catalog.ports = vec![capture].into_boxed_slice();
    let mut all_owned: FxHashSet<_> = (0..graph.nodes.len()).map(|index| NodeId::new(index as u32)).collect();
    assert!(all_owned.contains(&wire));
    assert!(all_owned.contains(&graph.contacts[contact.index()].node));

    let mut outside_wire = all_owned.clone();
    outside_wire.remove(&wire);
    let reads = super::settled::externally_read_contacts(&graph, &catalog, &outside_wire, &FxHashSet::default());
    assert!(reads.contains(&contact), "an outside captured wire reads the contact variant");
    assert!(super::settled::externally_observed(
        &graph,
        alternative,
        &outside_wire,
        &FxHashSet::default(),
        &reads,
    ));

    all_owned.insert(wire);
    let no_outside_read = super::settled::externally_read_contacts(&graph, &catalog, &all_owned, &FxHashSet::default());
    assert!(no_outside_read.is_empty());
    assert!(!super::settled::externally_observed(
        &graph,
        capture.kind,
        &all_owned,
        &FxHashSet::default(),
        &no_outside_read,
    ));
    assert!(!super::settled::externally_observed(
        &graph,
        alternative,
        &all_owned,
        &FxHashSet::default(),
        &no_outside_read,
    ));

    catalog.ports = Box::new([]);
    let no_capture = super::settled::externally_read_contacts(&graph, &catalog, &outside_wire, &FxHashSet::default());
    assert!(no_capture.is_empty());
    assert!(!super::settled::externally_observed(
        &graph,
        alternative,
        &outside_wire,
        &FxHashSet::default(),
        &no_capture,
    ));
}

#[test]
fn computation_feedback_leaf_uses_far_contact_as_canonical_state() {
    let monitor = TaskMonitor::default();
    let world = tiny_world();
    let mut graph = prepared_o6(&world, (BlockPos::new(1, 1, 1), BlockPos::new(3, 3, 3)), &monitor);
    let near = ContactId::new(graph.contacts.len() as u32);
    let far = ContactId::new(near.index() as u32 + 1);
    let resource = ResourceId::new(graph.resources.len() as u32);
    let position = BlockPos::new(0, 0, 0);
    for _ in [near, far] {
        graph.contacts.push(ContactSpec {
            node: NodeId::new(0),
            position,
            variants: Box::new([]),
        });
    }
    graph.resources.push(ResourceSpec {
        material: ProfileId::new(0),
        contacts: Box::new([near, far]),
    });
    let owner = PistonId::new(graph.pistons.len() as u32);
    graph.pistons.push(PistonSpec {
        node: NodeId::new(0),
        position,
        facing: BlockFace::Bottom,
        sticky: true,
        base: ContactId::new(graph.contacts.len() as u32),
        near,
        far,
        lane: PistonLane::SingleResource { far },
        routes: PistonRoutes {
            base_changed: RouteId::new(0),
            near_changed: RouteId::new(0),
            far_changed: None,
            far_shape_changed: None,
            base_restored: RouteId::new(0),
            head_restored: RouteId::new(0),
            resource_restored_near: None,
            resource_restored_far: None,
        },
    });

    let mut equations = Equations::new(&graph, &monitor).unwrap();
    equations.cells.insert(resource, (far, vec![owner]));
    equations.unique_cell_resources.insert(resource);

    let far_root = equations
        .feedback_resource_leaf(LayerInputSource::State(StateTest::ResourceAt { resource, contact: far }))
        .unwrap()
        .unwrap();
    let state_input = match equations.reduced.expression(far_root).unwrap() {
        LayerExpr::Input(input) => *input,
        expression => panic!("canonical Far feedback must be a direct state input, got {expression:?}"),
    };
    assert_eq!(
        equations.reduced.inputs[state_input.0 as usize].source,
        LayerInputSource::State(StateTest::ResourceAt { resource, contact: far })
    );

    let near_root = equations
        .feedback_resource_leaf(LayerInputSource::State(StateTest::ResourceAt { resource, contact: near }))
        .unwrap()
        .unwrap();
    assert_eq!(equations.reduced.expression(near_root).unwrap(), &LayerExpr::Constant(false));

    let near_source = LayerInputSource::State(StateTest::ResourceAt { resource, contact: near });
    equations.retain_classic = true;
    assert!(
        equations.source_value(near_source).is_err(),
        "legacy extraction must retain the explicit transfer requirement"
    );
    equations
        .sources
        .insert(LayerInputSource::State(StateTest::ResourceAt { resource, contact: far }), far_root);
    equations.feedback_resources = true;
    assert_eq!(
        equations.source_value(near_source).unwrap(),
        near_root,
        "the candidate tracks common-Far presence and ignores destination selection"
    );

    assert!(
        equations
            .feedback_resource_leaf(LayerInputSource::State(StateTest::ResourceAt {
                resource,
                contact: ContactId::new(far.index() as u32 + 1),
            }))
            .unwrap()
            .is_none()
    );
    equations.cells.insert(resource, (far, vec![owner, owner]));
    assert!(
        equations
            .feedback_resource_leaf(LayerInputSource::State(StateTest::ResourceAt { resource, contact: near }))
            .unwrap()
            .is_none()
    );

    assert!(
        compile_computation(&graph, &[], &monitor).is_err(),
        "empty public output sets must not produce a computation"
    );
}

#[test]
fn shared_far_feedback_presence_requires_every_owner_to_be_powered() {
    let monitor = TaskMonitor::default();
    let world = tiny_world();
    let graph = prepared_o6(&world, (BlockPos::new(1, 1, 1), BlockPos::new(3, 3, 3)), &monitor);
    let mut equations = Equations::new(&graph, &monitor).unwrap();
    let owner_a = equations
        .reduced
        .input(
            LayerInputSource::State(StateTest::ResourceEnabled(ResourceId::new(0))),
            LayerInputRole::RequestSample,
        )
        .unwrap();
    let owner_b = equations
        .reduced
        .input(
            LayerInputSource::State(StateTest::ResourceEnabled(ResourceId::new(1))),
            LayerInputRole::RequestSample,
        )
        .unwrap();
    let power_a = equations.reduced.input_expr(owner_a).unwrap();
    let power_b = equations.reduced.input_expr(owner_b).unwrap();
    let root = equations.shared_far_presence_from(vec![power_a, power_b]).unwrap();

    for (owners_powered, expected_common_far) in [([true, true], true), ([false, true], false), ([true, false], false)] {
        assert_eq!(
            evaluate_logical_expression(&equations.reduced, root, &owners_powered),
            expected_common_far,
            "owner power {owners_powered:?} must produce common-Far presence {expected_common_far}"
        );
    }
}

#[test]
fn raw_resource_lane_inference_accepts_shared_far_with_multiple_near_contacts() {
    let near = ContactId::new(10);
    let far = ContactId::new(11);
    let lane = |id, near, far, sticky| PistonSpec {
        node: NodeId::new(id),
        position: BlockPos::new(0, 0, id as i32),
        facing: BlockFace::Bottom,
        sticky,
        base: ContactId::new(id + 20),
        near,
        far,
        lane: PistonLane::SingleResource { far },
        routes: PistonRoutes {
            base_changed: RouteId::new(0),
            near_changed: RouteId::new(0),
            far_changed: None,
            far_shape_changed: None,
            base_restored: RouteId::new(0),
            head_restored: RouteId::new(0),
            resource_restored_near: None,
            resource_restored_far: None,
        },
    };

    let inferred = super::settled::infer_raw_resource_lane(&[near, far], &[lane(0, near, far, true)]).unwrap();
    assert_eq!(inferred, Some((far, vec![PistonId::new(0)])));
    assert_eq!(
        super::settled::infer_raw_resource_lane(&[near, far], &[lane(0, far, near, true)]).unwrap(),
        Some((near, vec![PistonId::new(0)])),
    );
    assert!(
        super::settled::infer_raw_resource_lane(&[near, far], &[lane(0, near, far, false)])
            .unwrap()
            .is_none()
    );
    assert!(super::settled::infer_raw_resource_lane(&[near, far], &[lane(0, near, far, true), lane(1, far, near, true)]).is_err());

    let near_b = ContactId::new(12);
    let shared = super::settled::infer_raw_resource_lane(&[near, near_b, far], &[lane(0, near, far, true), lane(1, near_b, far, true)]).unwrap();
    assert_eq!(shared, Some((far, vec![PistonId::new(0), PistonId::new(1)])));
    assert!(
        super::settled::infer_raw_resource_lane(
            &[near, near_b, far, ContactId::new(13)],
            &[lane(0, near, far, true), lane(1, near_b, far, true)],
        )
        .unwrap()
        .is_none()
    );

    let monitor = TaskMonitor::default();
    let world = tiny_world();
    let mut graph = prepared_o6(&world, (BlockPos::new(1, 1, 1), BlockPos::new(3, 3, 3)), &monitor);
    let candidate_near = ContactId::new(graph.contacts.len() as u32);
    let candidate_far = ContactId::new(candidate_near.index() as u32 + 1);
    let resource = ResourceId::new(graph.resources.len() as u32);
    let candidate_near_b = ContactId::new(candidate_far.index() as u32 + 1);
    for _ in [candidate_near, candidate_far, candidate_near_b] {
        graph.contacts.push(ContactSpec {
            node: NodeId::new(0),
            position: BlockPos::new(0, 0, 0),
            variants: Box::new([]),
        });
    }
    graph.resources.push(ResourceSpec {
        material: ProfileId::new(0),
        contacts: Box::new([candidate_near, candidate_far, candidate_near_b]),
    });
    let actor_index = graph.pistons.len() as u32;
    let actor = PistonId::new(actor_index);
    graph.pistons.push(lane(actor_index, candidate_near, candidate_far, true));
    let actor_b = PistonId::new(actor_index + 1);
    graph.pistons.push(lane(actor_index + 1, candidate_near_b, candidate_far, true));
    let mut equations = Equations::new(&graph, &monitor).unwrap();
    super::settled::install_candidate_resource_lane(&mut equations, resource, candidate_far, vec![actor, actor_b]);
    assert_eq!(
        equations.feedback_resource_contract(resource),
        Some((candidate_far, vec![actor, actor_b]))
    );
    assert!(equations.disabled_destinations.contains(&(resource, candidate_near)));
    assert!(equations.disabled_destinations.contains(&(resource, candidate_near_b)));
    assert!(
        equations
            .feedback_resource_leaf(LayerInputSource::State(StateTest::ResourceAt {
                resource,
                contact: candidate_far
            }))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        equations
            .feedback_resource_leaf(LayerInputSource::State(StateTest::ResourceAt {
                resource,
                contact: candidate_near
            }))
            .unwrap()
            .map(|root| equations.reduced.expression(root).unwrap().clone()),
        Some(LayerExpr::Constant(false)),
        "shared-Far feedback tracks a single canonical presence bit, not a winning destination"
    );
}

#[test]
fn candidate_torch_rule_is_boolean_inverse_of_main_input() {
    for main_powered in [false, true] {
        let torch_strength = if main_powered { 0 } else { 15 };
        for threshold in 0..=15 {
            assert_eq!(torch_strength > threshold, !main_powered && threshold < 15);
        }
    }
}

#[test]
fn candidate_comparator_threshold_equations_match_primitive_strength_rule() {
    let mut world = tiny_world();
    let source = BlockPos::new(4, 2, 4);
    let target = BlockPos::new(6, 2, 4);
    for position in [source, target] {
        world.set_block(position + BlockPos::new(0, -1, 0), Block::Stone {});
        world.set_block(
            position,
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::West, ComparatorMode::Compare, false),
            },
        );
    }
    let monitor = TaskMonitor::default();
    let mut graph = prepared_o6(&world, (source + BlockPos::new(-1, -1, -1), target + BlockPos::new(1, 2, 1)), &monitor);
    let source_node = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(source)).unwrap() as u32);
    let target_node = NodeId::new(graph.nodes.iter().position(|node| node.position == Some(target)).unwrap() as u32);
    let source_site = SiteId::new(
        graph
            .sites
            .iter()
            .position(|site| site.source == SiteSource::Component(source_node))
            .expect("source comparator has an output site") as u32,
    );
    let always = GuardId::new(graph.guards.iter().position(|guard| *guard == Guard::Always).unwrap() as u32);
    graph.power_links.retain(|link| {
        link.to
            != PowerTarget::Input {
                node: target_node,
                port: InputPort::Side,
            }
    });
    graph.power_links.push(PowerLink {
        from: source_site,
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: target_node,
            port: InputPort::Side,
        },
        face: BlockFace::West,
        attenuation: 0,
        guard: always,
    });

    for mode in [ComparatorMode::Compare, ComparatorMode::Subtract] {
        let mut equations = Equations::new(&graph, &monitor).unwrap();
        equations.retain_classic = true;
        let mut roots = Vec::new();
        for main in 0..=15u8 {
            roots.push(
                (0..=15u8)
                    .map(|threshold| equations.comparator_output_above(target_node, mode, threshold, Some(main)).unwrap())
                    .collect::<Vec<_>>(),
            );
        }
        for main in 0..=15u8 {
            for side in 0..=15u8 {
                let values = equations
                    .reduced
                    .inputs
                    .iter()
                    .map(|input| match input.source {
                        LayerInputSource::SourceAbove { site, threshold, .. } if site == source_site => side > threshold,
                        _ => false,
                    })
                    .collect::<Vec<_>>();
                let strength = crate::redpiler::power_rules::comparator_output_strength(main, side, mode);
                for (threshold, &root) in roots[main as usize].iter().enumerate() {
                    assert_eq!(
                        evaluate_logical_expression(&equations.reduced, root, &values),
                        strength > threshold as u8,
                        "mode={mode:?} main={main} side={side} threshold={threshold}"
                    );
                }
            }
        }
    }
}

#[test]
fn candidate_torch_source_inlines_the_main_input_inverse() {
    let mut world = tiny_world();
    let lever_pos = BlockPos::new(4, 2, 4);
    let torch_pos = BlockPos::new(5, 2, 4);
    world.set_block(lever_pos, Block::Lever { lever: Default::default() });
    world.set_block(torch_pos, Block::RedstoneTorch { lit: true });
    let monitor = TaskMonitor::default();
    let mut graph = prepared_o6(&world, (BlockPos::new(1, 1, 1), BlockPos::new(8, 4, 8)), &monitor);
    let lever_node = graph.nodes.iter().position(|node| node.position == Some(lever_pos)).unwrap();
    let torch_node = graph.nodes.iter().position(|node| node.position == Some(torch_pos)).unwrap();
    let lever_node = NodeId::new(lever_node as u32);
    let torch_node = NodeId::new(torch_node as u32);
    let site_for = |graph: &mut CompileGraph, node| {
        graph
            .sites
            .iter()
            .position(|site| site.source == SiteSource::Component(node))
            .map(|index| SiteId::new(index as u32))
            .unwrap_or_else(|| {
                let id = SiteId::new(graph.sites.len() as u32);
                graph.sites.push(SiteSpec {
                    position: graph.nodes[node.index()].position.unwrap(),
                    source: SiteSource::Component(node),
                    source_strength: 0,
                });
                id
            })
    };
    let lever_site = site_for(&mut graph, lever_node);
    let torch_site = site_for(&mut graph, torch_node);
    graph.power_links.push(PowerLink {
        from: lever_site,
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: torch_node,
            port: InputPort::Main,
        },
        face: BlockFace::Top,
        attenuation: 0,
        guard: GuardId::new(0),
    });

    let mut equations = Equations::new(&graph, &monitor).unwrap();
    equations.retain_classic = true;
    equations.feedback_resources = true;
    let root = equations
        .source(LayerInputSource::SourceAbove {
            site: torch_site,
            port: PowerPort::Output,
            threshold: 0,
        })
        .unwrap();
    let input = equations
        .reduced
        .inputs
        .iter()
        .position(|input| {
            input.source
                == (LayerInputSource::SourceAbove {
                    site: lever_site,
                    port: PowerPort::Output,
                    threshold: 0,
                })
        })
        .expect("torch main query should depend on the lever boundary") as u32;
    for powered in [false, true] {
        let mut values = vec![false; equations.reduced.inputs.len()];
        values[input as usize] = powered;
        assert_eq!(evaluate_logical_expression(&equations.reduced, root, &values), !powered);
    }
}

fn evaluate_logical_expression(builder: &LayerBuilder<'_>, root: LayerExprId, inputs: &[bool]) -> bool {
    match builder.expression(root).unwrap() {
        LayerExpr::Constant(value) => *value,
        LayerExpr::Input(input) => inputs[input.0 as usize],
        LayerExpr::Not(child) => !evaluate_logical_expression(builder, *child, inputs),
        LayerExpr::All(children) => children.iter().all(|&child| evaluate_logical_expression(builder, child, inputs)),
        LayerExpr::Any(children) => children.iter().any(|&child| evaluate_logical_expression(builder, child, inputs)),
    }
}
