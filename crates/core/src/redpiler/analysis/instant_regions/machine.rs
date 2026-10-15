//! Ahead-of-time partial evaluation of small, closed instant selections.
//! Every retained edge is an exact ordered Graph transition; missing edges do not speculate.
use super::*;
use crate::redpiler::backend::graph::GraphBackend;
use crate::redpiler::components::NodeType;
use rustc_hash::FxHashMap;
use std::sync::Arc;

const MAX_STATES: usize = 2048;
const MAX_BYTES: usize = 16 * 1024 * 1024;

fn controls(graph: &CompileGraph) -> Option<Vec<NodeId>> {
    let [region] = graph.instant_regions.as_slice() else {
        return None;
    };
    if !region.stable_inputs
        || graph.nodes.len() > 128
        || graph.pistons.is_empty()
        || graph.pistons.len() > 16
        || region.actors.len() != graph.pistons.len()
        || region.resources.len() != graph.resources.len()
        || region.contacts.len() != graph.contacts.len()
        || region.wires.len() != graph.wires.len()
    {
        return None;
    }
    let mut controls = Vec::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        match node.kind {
            CircuitNodeKind::Electrical(NodeType::Lever) => controls.push(NodeId::new(index as u32)),
            CircuitNodeKind::Electrical(NodeType::Constant | NodeType::Repeater { .. })
            | CircuitNodeKind::BuiltinReset { .. }
            | CircuitNodeKind::Wire(_)
            | CircuitNodeKind::Piston(_)
            | CircuitNodeKind::Contact(_) => {}
            // No host effects, outside observers, analog feedback or independently changing controls.
            _ => return None,
        }
    }
    (1..=4).contains(&controls.len()).then_some(controls)
}

fn evaluator(graph: &CompileGraph) -> Result<GraphBackend, String> {
    let mut base = graph.clone();
    base.instant_machine = None;
    let (program, state) = base.lower().map_err(|e| e.to_string())?;
    GraphBackend::new(program, state, crate::redpiler::backend::graph::MACHINE_EPOCH).map_err(|e| e.to_string())
}

pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Option<Arc<InstantMachine>>, String> {
    let Some(controls) = controls(graph) else {
        return Ok(None);
    };
    let mut sandbox = evaluator(graph)?;
    let mut initial = MachineState::default();
    if !sandbox.capture_machine_state(&mut initial) {
        return Ok(None);
    }
    let mut machine = InstantMachine {
        controls,
        states: vec![initial.clone()],
        transitions: Vec::new(),
        index: FxHashMap::default(),
    };
    machine.index.insert(initial, 0);
    machine.transitions.push(vec![None; machine.controls.len() + 1]);
    // Spend the bounded state budget on held-input histories; unknown histories stay general.
    // Repetition detects a periodic endpoint; no guessed "reset after N ticks" is used.
    for mask in 0..(1usize << machine.controls.len()) {
        for reverse in [false, true] {
            check_cancelled(monitor)?;
            let mut source = 0;
            for offset in 0..machine.controls.len() {
                let control = if reverse { machine.controls.len() - 1 - offset } else { offset };
                if mask & (1 << control) != 0 {
                    let Some(next) = evaluate(&mut machine, &mut sandbox, source, control + 1) else {
                        break;
                    };
                    source = next;
                }
            }
            let endpoint = follow_ticks(&mut machine, &mut sandbox, source, monitor)?;
            for action in 1..=machine.controls.len() {
                if let Some(next) = evaluate(&mut machine, &mut sandbox, endpoint, action) {
                    follow_ticks(&mut machine, &mut sandbox, next, monitor)?;
                }
            }
        }
    }
    if machine.estimated_bytes() > MAX_BYTES {
        return Ok(None);
    }
    check_cancelled(monitor)?;
    Ok(Some(Arc::new(machine)))
}

fn evaluate(machine: &mut InstantMachine, sandbox: &mut GraphBackend, source: usize, action: usize) -> Option<usize> {
    if let Some(edge) = &machine.transitions[source][action] {
        return Some(edge.next as usize);
    }
    let (state, mut edge) = sandbox.evaluate_machine_step(&machine.states[source], &machine.controls, action).ok()?;
    let next = if let Some(&existing) = machine.index.get(&state) {
        existing
    } else {
        if machine.states.len() == MAX_STATES {
            return None;
        }
        let next = machine.states.len() as u32;
        machine.index.insert(state.clone(), next);
        machine.states.push(state);
        machine.transitions.push(vec![None; machine.controls.len() + 1]);
        next
    };
    edge.next = next;
    machine.transitions[source][action] = Some(edge);
    Some(next as usize)
}

fn follow_ticks(machine: &mut InstantMachine, sandbox: &mut GraphBackend, mut source: usize, monitor: &TaskMonitor) -> Result<usize, String> {
    let mut seen = std::collections::HashSet::new();
    while seen.insert(source) {
        check_cancelled(monitor)?;
        let Some(next) = evaluate(machine, sandbox, source, 0) else { break };
        source = next;
    }
    Ok(source)
}

/// Independent replay validates both target memory and every compiled write/obligation.
pub(crate) fn validate(graph: &CompileGraph, machine: &InstantMachine) -> Result<(), GraphBuildError> {
    let invalid = |state, action| GraphBuildError::InvalidInstantMachine { state, action };
    if controls(graph).as_ref() != Some(&machine.controls)
        || machine.states.is_empty()
        || machine.states.len() > MAX_STATES
        || machine.estimated_bytes() > MAX_BYTES
        || machine.transitions.len() != machine.states.len()
        || machine.index.len() != machine.states.len()
    {
        return Err(invalid(0, 0));
    }
    let mut sandbox = evaluator(graph).map_err(|_| invalid(0, 0))?;
    for (source, state) in machine.states.iter().enumerate() {
        if machine.index.get(state) != Some(&(source as u32)) || machine.transitions[source].len() != machine.controls.len() + 1 {
            return Err(invalid(source, 0));
        }
        sandbox.restore_machine_state(state).map_err(|_| invalid(source, 0))?;
        let mut roundtrip = MachineState::default();
        if !sandbox.capture_machine_state(&mut roundtrip) || roundtrip != *state {
            return Err(invalid(source, 0));
        }
        for (action, edge) in machine.transitions[source].iter().enumerate() {
            let Some(edge) = edge else {
                continue;
            };
            let Some(expected) = machine.states.get(edge.next as usize) else {
                return Err(invalid(source, action));
            };
            let (actual, mut actual_edge) = sandbox
                .evaluate_machine_step(state, &machine.controls, action)
                .map_err(|_| invalid(source, action))?;
            actual_edge.next = edge.next;
            if &actual != expected || actual_edge != *edge {
                return Err(invalid(source, action));
            }
        }
    }
    Ok(())
}
