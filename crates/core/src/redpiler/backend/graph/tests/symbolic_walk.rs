//! One symbolic derivation must preserve every analog valuation's wire writes and callback order.
use super::*;
use crate::redpiler::analysis::instant_regions::compiler::{Compiler as SymbolicCompiler, Delivery, Receivers};
use mchprs_redpiler_opt::{
    Limits,
    symbolic::{Formula, Signal},
};

#[derive(Default)]
struct Boundary {
    /// These retain every boundary callback occurrence, including meaningful duplicates.
    deliveries: Vec<Delivery>,
}

impl Receivers for Boundary {
    fn deliver(
        &mut self,
        _: &mut SymbolicCompiler<'_>,
        delivery: Delivery,
    ) -> Result<(), crate::redpiler::analysis::instant_regions::compiler::CompileError> {
        self.deliveries.push(delivery);
        Ok(())
    }
}

#[test]
fn one_symbolic_dust_program_preserves_all_analog_transitions_and_conditional_callbacks() {
    assert_symbolic_transitions(&analog_dust_world());
}

#[test]
fn symbolic_branched_cycle_preserves_conditional_discovery_headings_and_fifo_order() {
    let mut world = analog_dust_world();
    for (x, z) in [(6, 5), (7, 5), (8, 5), (8, 6), (8, 7), (7, 7), (6, 7), (6, 6), (5, 6), (9, 6)] {
        let position = BlockPos::new(x, 20, z);
        world.set_block(position.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(position, Block::RedstoneWire { wire: Default::default() });
    }
    assert_symbolic_transitions(&world);
}

fn assert_symbolic_transitions(world: &PlotWorld) {
    let graph = passes::rich::prepare(world, BOUNDS, vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph;
    let (program, state) = graph.clone().lower().unwrap();
    let mut exact = GraphBackend::new(program, state, 0).unwrap();
    exact.set_test_recording(true);
    let input = exact.positions[&LEVER_POS];
    let wires: Vec<_> = graph.wires.iter().map(|wire| wire.node).collect();
    let root = exact.positions[&NEXT_POS];
    let mut compiler = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let old = Signal::from_bits(std::array::from_fn(|bit| compiler.logic.input(bit).unwrap()));
    let new = Signal::from_bits(std::array::from_fn(|bit| compiler.logic.input(4 + bit).unwrap()));
    let activate = compiler.logic.input(8).unwrap();
    compiler.state.components[input.index()].signal = new;
    compiler.state.components[input.index()].powered = compiler.logic.powered(new).unwrap();
    for (distance, &wire) in wires.iter().enumerate() {
        compiler.state.components[wire.index()].signal = compiler.logic.attenuate(old, distance as u8).unwrap();
    }
    compiler.state_written();
    let mut boundary = Boundary::default();
    compiler.seed_wire(root, activate, None, &mut boundary).unwrap();
    let mut roots = Vec::new();
    for &wire in &wires {
        roots.extend(compiler.state.components[wire.index()].signal.bits());
    }
    roots.extend(boundary.deliveries.iter().map(|delivery| delivery.when));
    let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
    let mut scratch = vec![false; plan.scratch_len()];
    let mut output = vec![false; roots.len()];
    for old in 0u8..16 {
        for new in 0u8..16 {
            for activate in [false, true] {
                exact.clear_test_traces();
                set_analog_dust_state(&mut exact, input, &wires, old, new);
                if activate {
                    exact.evaluate_seed(root, None).unwrap();
                }
                let inputs: Vec<_> = (0..8)
                    .map(|bit| (if bit < 4 { old } else { new }) & (1 << (bit % 4)) != 0)
                    .chain([activate])
                    .collect();
                plan.evaluate(&inputs, &mut scratch, &mut output).unwrap();
                for (index, &wire) in wires.iter().enumerate() {
                    let value = output[index * 4..index * 4 + 4]
                        .iter()
                        .enumerate()
                        .fold(0, |value, (bit, &set)| value | (u8::from(set) << bit));
                    assert_eq!(
                        value,
                        exact.wire_state(wire).power,
                        "old={old}, new={new}, activate={activate}, wire={wire:?}"
                    );
                }
                let callbacks: Vec<_> = boundary
                    .deliveries
                    .iter()
                    .zip(&output[wires.len() * 4..])
                    .filter_map(|(delivery, &when)| {
                        when.then(|| {
                            assert!(
                                matches!(delivery.receiver, CallbackReceiver::Component(_)),
                                "this fixture has component boundary receivers only"
                            );
                            (graph.sites[delivery.target.index()].position, delivery.kind)
                        })
                    })
                    .collect();
                assert_eq!(
                    callbacks, exact.test_recorder.callback_order,
                    "ordered boundary old={old}, new={new}, activate={activate}"
                );
            }
        }
    }
    assert!(!boundary.deliveries.is_empty(), "boundary effects must survive scalar reduction");
    assert!(
        compiler.logic.node_count() < 50_000,
        "this is a shared expression graph, not 512 stored trajectories"
    );
}

#[test]
fn constant_propagation_still_obeys_the_semantic_work_budget() {
    let graph = passes::rich::prepare(&analog_dust_world(), BOUNDS, vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph;
    let mut compiler = SymbolicCompiler::new(&graph, Limits { max_work: 0, max_nodes: 100 }).unwrap();
    let mut boundary = Boundary::default();
    let error = compiler.seed_wire(graph.wires[0].node, Formula::TRUE, None, &mut boundary).unwrap_err();
    assert_eq!(
        error,
        crate::redpiler::analysis::instant_regions::compiler::CompileError::Logic(mchprs_redpiler_opt::symbolic::SymbolicError::BudgetExceeded)
    );
}

#[test]
fn symbolic_captured_wire_query_reads_new_comparator_output_and_old_dust_strengths() {
    use crate::redpiler::analysis::instant_regions::compiler::Capture;
    let graph = passes::rich::prepare(&analog_dust_world(), BOUNDS, vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph;
    let (program, state) = graph.clone().lower().unwrap();
    let mut exact = GraphBackend::new(program, state, 0).unwrap();
    let input = exact.positions[&LEVER_POS];
    let root = exact.positions[&NEXT_POS];
    let wires: Vec<_> = graph.wires.iter().map(|wire| wire.node).collect();
    let mut compiler = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let old = Signal::from_bits(std::array::from_fn(|bit| compiler.logic.input(bit).unwrap()));
    let new = Signal::from_bits(std::array::from_fn(|bit| compiler.logic.input(4 + bit).unwrap()));
    compiler.state.components[input.index()].signal = old;
    for &wire in &wires {
        compiler.state.components[wire.index()].signal = old;
    }
    compiler.state_written();
    let wire = graph.wires.iter().find(|wire| wire.node == root).unwrap();
    let mut capture = Capture::default();
    for site in wire.canonical_neighbors {
        capture.insert(site, compiler.observe(site).unwrap());
    }
    compiler.state.components[input.index()].signal = new;
    for &wire in &wires {
        compiler.state.components[wire.index()].signal = Signal::literal(0).unwrap();
    }
    compiler.state_written();
    let power = compiler.captured_wire_power(root, &capture, 3).unwrap();
    let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&power.bits()).unwrap(), Limits::default(), || false).unwrap();
    let mut scratch = vec![false; plan.scratch_len()];
    for old in 0u8..16 {
        for new in 0u8..16 {
            for &wire in &wires {
                let ComponentState::Wire(value) = &mut exact.state.components[wire.index()] else {
                    unreachable!()
                };
                value.power = old;
            }
            exact.set_node(input, old > 0, old, None, false).unwrap();
            let mut walk = captured_wire_walk(&exact, wire);
            let neighbors = exact.identify_wire_neighbors(root, &mut walk);
            exact.set_node(input, new > 0, new, None, false).unwrap();
            for &wire in &wires {
                let ComponentState::Wire(value) = &mut exact.state.components[wire.index()] else {
                    unreachable!()
                };
                value.power = 0;
            }
            let inputs: Vec<_> = (0..8).map(|bit| (if bit < 4 { old } else { new }) & (1 << (bit % 4)) != 0).collect();
            let mut output = [false; 4];
            plan.evaluate(&inputs, &mut scratch, &mut output).unwrap();
            let result = output.iter().enumerate().fold(0, |value, (bit, &set)| value | (u8::from(set) << bit));
            assert_eq!(
                result,
                exact.turbo_wire_power_general(wire, &neighbors, &walk).0,
                "captured old={old}, live new={new}"
            );
        }
    }
}
