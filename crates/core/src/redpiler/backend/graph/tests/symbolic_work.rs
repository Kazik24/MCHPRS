//! Conditional queue membership must never become global deduplication of equal expressions.
use super::*;
use crate::redpiler::analysis::instant_regions::compiler::{
    Compiler as SymbolicCompiler,
    work::{Request, Work},
};
use mchprs_redpiler_opt::{Limits, symbolic::Formula};
use mchprs_world::PistonAction;

fn graph() -> CompileGraph {
    passes::rich::prepare(&analog_dust_world(), BOUNDS, vec![], &TaskMonitor::default(), AnalysisLimits::default())
        .unwrap()
        .graph
}

#[test]
fn guarded_request_deduplication_is_scoped_to_matching_occurrences_still_queued() {
    let graph = graph();
    let mut compiler = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let a = compiler.logic.input(0).unwrap();
    let b = compiler.logic.input(1).unwrap();
    let mut work = Work::empty(graph.nodes.len());
    let extend = Request {
        actor: PistonId::new(0),
        action: PistonAction::Extend,
    };
    let retract = Request {
        action: PistonAction::Retract,
        ..extend
    };
    let other = Request {
        actor: PistonId::new(1),
        ..extend
    };
    work.request(&mut compiler, extend, a).unwrap();
    work.request(&mut compiler, extend, b).unwrap();
    work.request(&mut compiler, retract, b).unwrap();
    work.request(&mut compiler, other, Formula::TRUE).unwrap();
    let first = work.requests.pop_front().unwrap();
    // Consuming the first occurrence frees only its valuations; a later conditional duplicate remains.
    work.request(&mut compiler, extend, Formula::TRUE).unwrap();
    let mut occurrences = vec![first];
    occurrences.extend(work.requests);
    let roots: Vec<_> = occurrences.iter().map(|value| value.when).collect();
    let plan = mchprs_redpiler_opt::optimize(&compiler.logic.circuit(&roots).unwrap(), Limits::default(), || false).unwrap();
    for inputs in [[false, false], [false, true], [true, false], [true, true]] {
        let mut output = vec![false; roots.len()];
        plan.evaluate(&inputs, &mut vec![false; plan.scratch_len()], &mut output).unwrap();
        let actual: Vec<_> = occurrences
            .iter()
            .zip(output)
            .filter_map(|(value, selected)| selected.then_some(value.value))
            .collect();
        let mut queued = VecDeque::new();
        for (request, active) in [(extend, inputs[0]), (extend, inputs[1]), (retract, inputs[1]), (other, true)] {
            if active && !queued.contains(&request) {
                queued.push_back(request);
            }
        }
        let first = inputs[0].then(|| queued.pop_front().unwrap());
        if !queued.contains(&extend) {
            queued.push_back(extend);
        }
        let expected: Vec<_> = first.into_iter().chain(queued).collect();
        assert_eq!(actual, expected, "inputs {inputs:?}");
    }
}

#[test]
fn symbolic_scheduler_preserves_priority_conditional_pending_and_rearming() {
    let graph = graph();
    let mut compiler = SymbolicCompiler::new(&graph, Limits::default()).unwrap();
    let a = compiler.logic.input(0).unwrap();
    let mut work = Work::empty(graph.nodes.len());
    let node = NodeId::new(0);
    let other = NodeId::new(1);
    work.schedule(&mut compiler, node, 0, TickPriority::NanoTick, a).unwrap();
    work.schedule(&mut compiler, node, 1, TickPriority::Normal, Formula::TRUE).unwrap();
    work.schedule(&mut compiler, other, 1, TickPriority::High, Formula::TRUE).unwrap();
    let first = work.pop_tick(&mut compiler).unwrap().unwrap();
    assert_eq!(first.when, a);
    assert_eq!(first.value, node);
    // The other valuations still have their later occurrence and must not be rearmed.
    let rearmed = work.schedule(&mut compiler, node, 1, TickPriority::Normal, Formula::TRUE).unwrap();
    assert_eq!(rearmed, a);
    assert_eq!(work.pending[node.index()], Formula::TRUE);
    assert!(work.pop_tick(&mut compiler).unwrap().is_none());
    work.scheduler.end_last_tick_move_next();
    let high = work.pop_tick(&mut compiler).unwrap().unwrap();
    let normal = work.pop_tick(&mut compiler).unwrap().unwrap();
    let rearmed = work.pop_tick(&mut compiler).unwrap().unwrap();
    assert_eq!((high.value, high.when), (other, Formula::TRUE));
    assert_eq!((normal.value, normal.when), (node, compiler.logic.not(a).unwrap()));
    assert_eq!((rearmed.value, rearmed.when), (node, a));
    assert!(work.pop_tick(&mut compiler).unwrap().is_none());
    assert_eq!(work.pending, vec![Formula::FALSE; graph.nodes.len()]);
}
