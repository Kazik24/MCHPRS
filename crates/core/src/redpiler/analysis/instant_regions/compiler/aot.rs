//! Build immutable symbolic phase programs from the graph's ordered execution semantics.
use super::cycle::Snapshot;
use super::entry::Entry;
use super::execution::Execution;
use super::*;
use crate::redpiler::{TaskMonitor, program_graph::SymbolicKernel};
use mchprs_redpiler_opt::{Limits, optimize};
use std::sync::Arc;

const MAX_PROOF_TICKS: usize = 48;

/// Compile every lever entering a stable instant region whose recurrence can be proven.
pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> std::result::Result<Vec<Arc<SymbolicKernel>>, String> {
    let candidates = activation_candidates(graph);
    let mut kernels = Vec::new();
    for trigger in candidates {
        if monitor.cancelled() {
            return Err("cancelled while compiling symbolic instant kernels".into());
        }
        if let Some(kernel) = compile_candidate(graph, trigger, monitor)? {
            kernels.push(Arc::new(kernel));
        }
    }
    Ok(kernels)
}

fn activation_candidates(graph: &CompileGraph) -> Vec<NodeId> {
    let mut found = vec![false; graph.nodes.len()];
    for region in graph.plans.instant.regions.iter().filter(|region| region.stable_inputs) {
        for occurrence in &region.incoming {
            let Some(route) = graph.routes.get(occurrence.route.index()) else {
                continue;
            };
            let source = route.source;
            if matches!(
                graph.nodes[source.index()].kind,
                CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever)
            ) {
                found[source.index()] = true;
            }
        }
    }
    found
        .into_iter()
        .enumerate()
        .filter_map(|(index, found)| found.then_some(NodeId::new(index as u32)))
        .collect()
}

fn compile_candidate(graph: &CompileGraph, trigger: NodeId, monitor: &TaskMonitor) -> std::result::Result<Option<SymbolicKernel>, String> {
    let limits = Limits {
        max_work: 256 * 1024 * 1024,
        ..Limits::default()
    };
    let Ok(mut compiler) = Compiler::new(graph, limits) else {
        return Ok(None);
    };
    let Ok(entry) = Entry::derive(&mut compiler, trigger) else {
        return Ok(None);
    };

    let mut roots = Vec::new();
    let conditions = append_roots(&mut roots, entry.conditions.iter().copied());
    let entry_state = append_roots(&mut roots, compiler.state.scalar_roots());
    let Ok(revisions) = compiler.state.revision_counts(&mut compiler.logic) else {
        return Ok(None);
    };
    let entry_revisions = revisions.iter().map(|bits| append_roots(&mut roots, bits.iter().copied())).collect();

    let mut execution = Execution::empty(graph);
    let first_effect = execution.accepted.len();
    if execution.set_control(&mut compiler, trigger, Formula::FALSE).is_err() {
        return Ok(None);
    }
    let activation_snapshot = match Snapshot::capture(&mut compiler, &execution, first_effect) {
        Ok(snapshot) => snapshot,
        Err(_) if monitor.cancelled() => return Err("cancelled while compiling symbolic activation".into()),
        Err(_) => return Ok(None),
    };
    let activation = activation_snapshot.append_roots(&mut roots);
    let mut snapshots = vec![activation_snapshot];
    let mut phases = Vec::new();
    let mut recurrence = None;

    for _ in 1..=MAX_PROOF_TICKS {
        if monitor.cancelled() {
            return Err("cancelled while proving symbolic instant recurrence".into());
        }
        let first_effect = execution.accepted.len();
        if execution.run_tick(&mut compiler).is_err() {
            return Ok(None);
        }
        let snapshot = match Snapshot::capture(&mut compiler, &execution, first_effect) {
            Ok(snapshot) => snapshot,
            Err(_) if monitor.cancelled() => return Err("cancelled while capturing symbolic phase".into()),
            Err(_) => return Ok(None),
        };
        phases.push(snapshot.append_roots(&mut roots));
        for (prefix, earlier) in snapshots.iter().enumerate() {
            match earlier.proves_same(&snapshot, &mut compiler) {
                Ok(true) => {
                    recurrence = Some((prefix, phases.len() - prefix));
                    break;
                }
                Ok(false) => {}
                Err(_) if monitor.cancelled() => return Err("cancelled while proving symbolic recurrence".into()),
                Err(_) => return Ok(None),
            }
        }
        if recurrence.is_some() {
            break;
        }
        snapshots.push(snapshot);
    }
    let Some((prefix, period)) = recurrence else {
        return Ok(None);
    };

    let circuit = match compiler.logic.circuit(&roots) {
        Ok(circuit) => circuit,
        Err(_) if monitor.cancelled() => return Err("cancelled while lowering symbolic roots".into()),
        Err(_) => return Ok(None),
    };
    let plan = match optimize(&circuit, limits, || monitor.cancelled()) {
        Ok(plan) => plan,
        Err(_) if monitor.cancelled() => return Err("cancelled while optimizing symbolic kernel".into()),
        Err(_) => return Ok(None),
    };
    if phases.len() != prefix + period {
        return Ok(None);
    }
    let kernel = SymbolicKernel {
        trigger,
        inputs: entry.inputs.into_boxed_slice(),
        shapes: entry.shapes.into_boxed_slice(),
        hot_components: SymbolicKernel::component_bindings(graph),
        plan,
        entry: KernelEntryLayout {
            state: entry_state,
            conditions,
            revision_deltas: entry_revisions,
        },
        activation,
        phases: phases.into_boxed_slice(),
        prefix,
        period,
    };
    Ok(kernel.validate(graph).then_some(kernel))
}

fn append_roots(roots: &mut Vec<Formula>, values: impl IntoIterator<Item = Formula>) -> std::ops::Range<usize> {
    let start = roots.len();
    roots.extend(values);
    start..roots.len()
}
