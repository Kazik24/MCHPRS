//! Whole graph event semantics are derived once; concrete cases are only independent validation.
use super::*;
use crate::redpiler::analysis::instant_regions::compiler::{Compiler as SymbolicCompiler, execution::Execution};
use crate::redpiler::components::NodeType;
use mchprs_redpiler_opt::{Limits, symbolic::Formula};

pub(super) fn state_roots(compiler: &SymbolicCompiler<'_>) -> Vec<Formula> {
    compiler.state.scalar_roots()
}

pub(super) fn exact_state(backend: &GraphBackend) -> Vec<bool> {
    let mut values = Vec::new();
    for state in &backend.state.components {
        let (power, powered, locked) = match state {
            ComponentState::Electrical(state) => (state.output_strength, state.powered, state.repeater_locked),
            ComponentState::Wire(wire) => (wire.power, false, false),
            ComponentState::Specialized => (0, false, false),
        };
        values.extend((0..4).map(|bit| power & (1 << bit) != 0));
        values.extend([powered, locked]);
        if let ComponentState::Wire(wire) = state {
            for side in [wire.north, wire.south, wire.east, wire.west] {
                values.extend([side == RedstoneWireSide::Up, side == RedstoneWireSide::Side]);
            }
        }
    }
    for actor in &backend.state.pistons {
        values.extend([actor.extended, actor.head_present]);
        values.extend((0..5).map(|index| index == actor.availability as usize));
    }
    for (state, spec) in backend.state.resources.iter().zip(backend.program.resources()) {
        values.extend(spec.contacts.iter().map(|&contact| state.at == contact));
        values.push(state.enabled);
    }
    values
}

#[test]
fn one_symbolic_adder_episode_preserves_all_control_valuations_memory_and_future_work() {
    assert_symbolic_episode("adder_1bit", 4, &(0..16).collect::<Vec<_>>());
}

#[test]
fn eleven_bit_event_compilation_keeps_all_inputs_symbolic_and_matches_full_state_samples() {
    let all = (1 << 23) - 1;
    let mut masks = vec![0, all, 0x55_5555, 0x2a_aaaa];
    for bit in 0..23 {
        masks.extend([1 << bit, all ^ (1 << bit)]);
    }
    assert_symbolic_episode("adder_11bits", 23, &masks);
}

fn assert_symbolic_episode(id: &'static str, width: usize, masks: &[u32]) {
    let (graph, _) = stable_region_tests::stable_graph_at_level(id, 1);
    let controls: Vec<_> = graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| (node.kind == CircuitNodeKind::Electrical(NodeType::Lever)).then_some(NodeId::new(index as u32)))
        .collect();
    assert_eq!(controls.len(), width);
    // Whole-event construction visits guards repeatedly; this enlarges work only, not storage or input valuations.
    let limits = Limits {
        max_work: 256 * 1024 * 1024,
        ..Limits::default()
    };
    let mut compiler = SymbolicCompiler::new(&graph, limits).unwrap();
    let mut execution = Execution::empty(&graph);
    for (input, &node) in controls.iter().enumerate() {
        let variable = compiler.logic.input(input).unwrap();
        execution.set_control(&mut compiler, node, variable).unwrap();
    }
    for tick in 1..=12 {
        if let Err(error) = execution.run_tick(&mut compiler) {
            panic!(
                "{id} tick {tick}: {error:?}, decisions={}, Boolean work={}, remaining semantic work={}",
                compiler.logic.node_count(),
                compiler.logic.work(),
                compiler.remaining_work()
            );
        }
    }
    let mut roots = state_roots(&compiler);
    let state_count = roots.len();
    let revisions: Vec<_> = compiler
        .state
        .actors
        .iter()
        .map(|actor| &actor.revision)
        .chain(compiler.state.resources.iter().map(|resource| &resource.revision))
        .collect();
    let revisions_count = revisions.iter().map(|revision| revision.writes.len()).sum::<usize>();
    for revision in &revisions {
        roots.extend(revision.writes.iter().copied());
    }
    roots.extend(execution.work.pending.iter().copied());
    let scheduled: Vec<_> = execution.work.scheduler.iter().collect();
    roots.extend(scheduled.iter().map(|(event, _, _)| event.when));
    roots.extend(execution.work.requests.iter().map(|event| event.when));
    roots.extend(execution.publications.iter().map(|event| event.when));
    for event in &execution.accepted {
        roots.extend([event.when, event.previous_extended]);
    }
    let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
    let mut scratch = vec![false; plan.scratch_len()];
    let mut output = vec![false; roots.len()];
    eprintln!(
        "{id}: symbolic inputs={width}, decisions={}, plan_ops={}, roots={}, accepted_occurrences={}",
        compiler.logic.node_count(),
        plan.ops().len(),
        roots.len(),
        execution.accepted.len()
    );
    for &mask in masks {
        let inputs: Vec<_> = (0..width).map(|bit| mask & (1 << bit) != 0).collect();
        let (program, state) = graph.clone().lower().unwrap();
        let mut exact = GraphBackend::new(program, state, 0).unwrap();
        for (&node, &powered) in controls.iter().zip(&inputs) {
            if exact.node_state(node).powered != powered {
                exact.use_block(exact.position(node)).unwrap();
            }
        }
        let mut effects = Vec::new();
        for tick in 1..=12 {
            let before = exact.effects.len();
            exact.tick().unwrap();
            for effect in &exact.effects[before..] {
                let PresentationEffect::BlockAction {
                    position,
                    action: crate::world::BlockAction::Piston { action, piston },
                } = effect
                else {
                    panic!("this fixture emits only piston block actions");
                };
                let actor = graph.pistons.iter().position(|actor| actor.position == *position).unwrap();
                effects.push((PistonId::new(actor as u32), *action, piston.extended, tick));
            }
        }
        plan.evaluate(&inputs, &mut scratch, &mut output).unwrap();
        let expected = exact_state(&exact);
        let mismatch = output[..state_count].iter().zip(&expected).position(|(a, b)| a != b);
        assert!(mismatch.is_none(), "control mask {mask}, first differing state bit {mismatch:?}");
        let mut offset = state_count;
        let expected_revisions = exact
            .state
            .pistons
            .iter()
            .map(|state| state.revision)
            .chain(exact.state.resources.iter().map(|state| state.revision));
        for (revision, expected) in revisions.iter().zip(expected_revisions) {
            let increments = output[offset..offset + revision.writes.len()]
                .iter()
                .filter(|&&selected| selected)
                .count() as u64;
            assert_eq!(revision.base.wrapping_add(increments), expected, "revision mask {mask}");
            offset += revision.writes.len();
        }
        assert_eq!(offset, state_count + revisions_count);
        for index in 0..graph.nodes.len() {
            assert_eq!(
                output[offset],
                graph.nodes[index].raw_state.is_some() && exact.is_pending(NodeId::new(index as u32)),
                "pending mask {mask}, node {index}"
            );
            offset += 1;
        }
        let actual_scheduled: Vec<_> = scheduled
            .iter()
            .zip(&output[offset..offset + scheduled.len()])
            .filter_map(|((event, delay, priority), &when)| when.then_some((event.value, *delay, *priority)))
            .collect();
        let expected_scheduled: Vec<_> = exact
            .future
            .scheduler
            .iter()
            .map(|(event, delay, priority)| (event.node.unwrap(), delay, priority))
            .collect();
        assert_eq!(actual_scheduled, expected_scheduled, "scheduled mask {mask}");
        offset += scheduled.len();
        let actual_requests: Vec<_> = execution
            .work
            .requests
            .iter()
            .zip(&output[offset..offset + execution.work.requests.len()])
            .filter_map(|(event, &when)| when.then_some((event.value.actor, event.value.action)))
            .collect();
        let expected_requests: Vec<_> = exact.future.piston_requests.iter().map(|event| (event.piston, event.action)).collect();
        assert_eq!(actual_requests, expected_requests, "requests mask {mask}");
        offset += execution.work.requests.len();
        let actual_publications: Vec<_> = execution
            .publications
            .iter()
            .zip(&output[offset..offset + execution.publications.len()])
            .filter_map(|(event, &when)| when.then_some((event.actor, event.due_tick, event.extending)))
            .collect();
        let expected_publications: Vec<_> = exact
            .future
            .deferred_pistons
            .iter()
            .map(|event| (event.piston, event.due_tick, event.extending))
            .collect();
        assert_eq!(actual_publications, expected_publications, "publications mask {mask}");
        offset += execution.publications.len();
        let actual_effects: Vec<_> = execution
            .accepted
            .iter()
            .zip(output[offset..].chunks_exact(2))
            .filter_map(|(event, bits)| bits[0].then_some((event.request.actor, event.request.action, bits[1], event.tick)))
            .collect();
        assert_eq!(actual_effects, effects, "ordered accepted block actions mask {mask}");
    }
}
