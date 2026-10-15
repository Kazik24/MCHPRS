//! Build immutable symbolic phase programs from the graph's ordered execution semantics.
use super::cycle::Snapshot;
use super::entry::{Entry, ShapeEntry};
use super::execution::Execution;
use super::*;
use crate::redpiler::{TaskMonitor, program_graph::SymbolicKernel};
use mchprs_redpiler_opt::{Limits, optimize};
use std::sync::Arc;

const MAX_PROOF_TICKS: usize = 48;

/// Compile both control edges where complete state and ordered future recurrence can be proven.
pub(crate) fn compile(graph: &CompileGraph, monitor: &TaskMonitor) -> std::result::Result<Vec<Arc<SymbolicKernel>>, String> {
    let candidates = activation_candidates(graph);
    let mut kernels = Vec::new();
    for trigger in candidates {
        for powered_after in [false, true] {
            if monitor.cancelled() {
                return Err("cancelled while compiling symbolic instant kernels".into());
            }
            let result = compile_candidate(graph, trigger, powered_after, monitor);
            if monitor.cancelled() {
                return Err("cancelled while compiling symbolic instant kernels".into());
            }
            match result {
                Ok(kernel) => kernels.push(Arc::new(kernel)),
                Err(reason) => {
                    tracing::debug!(?trigger, position = ?graph.nodes[trigger.index()].position, powered_after, %reason, "symbolic kernel declined")
                }
            }
        }
    }
    Ok(kernels)
}

pub(crate) fn activation_candidates(graph: &CompileGraph) -> Vec<NodeId> {
    if !graph.plans.instant.regions.iter().any(|region| region.stable_inputs) {
        return Vec::new();
    }
    // A control can reach an instant through wires or diodes, without a direct incoming callback.
    // The complete-state proof, rather than adjacency, decides whether its program is safe.
    graph
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(node.kind, CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever)).then_some(NodeId::new(index as u32))
        })
        .collect()
}

pub(crate) fn compile_candidate(
    graph: &CompileGraph,
    trigger: NodeId,
    powered_after: bool,
    monitor: &TaskMonitor,
) -> std::result::Result<SymbolicKernel, String> {
    let mut declines = Vec::new();
    for shapes in [ShapeEntry::Histories, ShapeEntry::Captured] {
        match compile_for_entry(graph, trigger, powered_after, shapes, monitor) {
            Ok(kernel) => return Ok(kernel),
            Err(reason) => declines.push(format!("{shapes:?}: {reason}")),
        }
        if monitor.cancelled() {
            return Err("cancelled while compiling symbolic instant kernels".into());
        }
    }
    Err(declines.join("; "))
}

pub(crate) fn compile_for_entry(
    graph: &CompileGraph,
    trigger: NodeId,
    powered_after: bool,
    shapes: ShapeEntry,
    monitor: &TaskMonitor,
) -> std::result::Result<SymbolicKernel, String> {
    let limits = Limits {
        max_work: (256usize * 1024 * 1024).saturating_mul(monitor.budget_multiplier()),
        max_nodes: Limits::default().max_nodes.saturating_mul(monitor.budget_multiplier()),
    };
    let mut compiler = Compiler::new(graph, limits).map_err(|error| format!("initial state: {error:?}"))?;
    let entry = Entry::derive_for_shapes(&mut compiler, trigger, !powered_after, shapes)
        .map_err(|error| proof_error("ready entry", error, &compiler, limits))?;

    let mut roots = Vec::new();
    let conditions = append_roots(&mut roots, entry.conditions.iter().copied());
    let entry_state = append_roots(&mut roots, compiler.state.scalar_roots());
    let revisions = compiler
        .state
        .revision_counts(&mut compiler.logic)
        .map_err(|error| format!("entry revisions: {error:?}"))?;
    let entry_revisions = revisions.iter().map(|bits| append_roots(&mut roots, bits.iter().copied())).collect();

    let mut execution = Execution::empty(graph);
    let first_effect = execution.accepted.len();
    execution
        .set_control(&mut compiler, trigger, if powered_after { Formula::TRUE } else { Formula::FALSE })
        .map_err(|error| proof_error("activation", error, &compiler, limits))?;
    let activation_snapshot = Snapshot::capture(&mut compiler, &execution, first_effect).map_err(|error| format!("activation roots: {error:?}"))?;
    let activation = activation_snapshot.append_roots(&mut roots);
    let mut snapshots = vec![activation_snapshot];
    let mut phases = Vec::new();
    let mut recurrence = None;

    for tick in 1..=MAX_PROOF_TICKS {
        if monitor.cancelled() {
            return Err("cancelled while proving symbolic instant recurrence".into());
        }
        let first_effect = execution.accepted.len();
        execution
            .run_tick(&mut compiler)
            .map_err(|error| proof_error(&format!("tick {tick}"), error, &compiler, limits))?;
        let snapshot = Snapshot::capture(&mut compiler, &execution, first_effect).map_err(|error| format!("tick {tick} roots: {error:?}"))?;
        phases.push(snapshot.append_roots(&mut roots));
        for (prefix, earlier) in snapshots.iter().enumerate() {
            match earlier.proves_same(&snapshot, &mut compiler) {
                Ok(true) => {
                    recurrence = Some((prefix, phases.len() - prefix));
                    break;
                }
                Ok(false) => {}
                Err(error) => return Err(format!("tick {tick} recurrence: {error:?}")),
            }
        }
        if recurrence.is_some() {
            break;
        }
        snapshots.push(snapshot);
    }
    let Some((prefix, period)) = recurrence else {
        return Err(format!("no complete-state recurrence within {MAX_PROOF_TICKS} ticks"));
    };

    let circuit = compiler.logic.circuit(&roots).map_err(|error| format!("lowering roots: {error:?}"))?;
    let plan = optimize(&circuit, limits, || monitor.cancelled()).map_err(|error| format!("Boolean optimization: {error:?}"))?;
    if phases.len() != prefix + period {
        return Err("invalid recurrence layout".into());
    }
    let kernel = SymbolicKernel {
        trigger,
        powered_after,
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
    if !kernel.validate(graph) {
        return Err("invalid kernel root layout or binding".into());
    }
    Ok(kernel)
}

fn proof_error(stage: &str, error: CompileError, compiler: &Compiler<'_>, limits: Limits) -> String {
    let detail = match error {
        CompileError::UnsupportedNode(node) => match compiler.graph.nodes.get(node.index()) {
            Some(node) => format!("unsupported {:?} at {:?}", node.kind, node.position),
            None => format!("unknown node {node:?}"),
        },
        error => format!("{error:?}"),
    };
    format!(
        "{stage}: {detail}; decisions={}, logic work={}, semantic work={}",
        compiler.logic.node_count(),
        compiler.logic.work(),
        limits.max_work - compiler.remaining_work()
    )
}

fn append_roots(roots: &mut Vec<Formula>, values: impl IntoIterator<Item = Formula>) -> std::ops::Range<usize> {
    let start = roots.len();
    roots.extend(values);
    start..roots.len()
}
