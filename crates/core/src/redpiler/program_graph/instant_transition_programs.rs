//! Static eligibility for exact ordered transport-episode programs.
//!
//! The runtime may record an episode with the existing dense executor and later
//! replay it only when its complete entry conditions match. This product only
//! binds each transport wire to its validated typed drive query and certifies
//! that every retained internal callback slot has a fixed admission decision.
//! External callback admission remains live in the recorded transcript.
//!
//! This is an eligibility product, not a claim that transport or callback work
//! has been eliminated. The transcript must preserve all capture and call cuts.

use super::*;
use crate::redpiler::TaskMonitor;
use std::collections::BTreeMap;

const MAX_TRANSITION_PROGRAM_WORK: usize = 8_000_000;
const MAX_TRANSITION_PROGRAM_BYTES: usize = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 8;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantTransitionPrograms {
    /// Rows align with the certified transport nets.
    pub(crate) nets: Vec<Option<InstantTransitionNet>>,
    pub(crate) stats: InstantTransitionProgramStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantTransitionProgramStats {
    pub(crate) eligible_nets: usize,
    pub(crate) eligible_wires: usize,
    pub(crate) declined_nets: usize,
    pub(crate) work: usize,
    pub(crate) bytes: usize,
    pub(crate) declines: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InstantTransitionNet {
    /// Existing typed no-dust drive query for each transport-local wire.
    pub(crate) wire_queries: Box<[u32]>,
}

/// Re-derive the eligibility table from typed source plans and transport routes.
pub(crate) fn infer_instant_transition_programs(
    graph: &CompileGraph,
    transport: &InstantTransport,
    drives: &InstantDriveProgram,
    monitor: &TaskMonitor,
) -> Result<InstantTransitionPrograms, String> {
    if monitor.cancelled() {
        return Err("cancelled compiling instant transition programs".into());
    }
    let mut result = InstantTransitionPrograms {
        nets: vec![None; transport.nets.len()],
        ..Default::default()
    };
    let mut work = 0usize;
    let mut bytes = transport.nets.len().saturating_mul(std::mem::size_of::<Option<InstantTransitionNet>>());

    for (net_index, net) in transport.nets.iter().enumerate() {
        if monitor.cancelled() {
            return Err("cancelled compiling instant transition programs".into());
        }
        let net_work = net.wires.len().saturating_mul(25);
        if work.saturating_add(net_work) > MAX_TRANSITION_PROGRAM_WORK {
            decline(&mut result.stats, "work_budget");
            continue;
        }
        work += net_work;
        let row_bytes = net.wires.len().saturating_mul(std::mem::size_of::<u32>());
        if bytes.saturating_add(row_bytes) > MAX_TRANSITION_PROGRAM_BYTES {
            decline(&mut result.stats, "memory_budget");
            continue;
        }

        let mut wire_queries = Vec::with_capacity(net.wires.len());
        let mut failure = None;
        for (local_wire, wire) in net.wires.iter().enumerate() {
            if local_wire % 256 == 0 && monitor.cancelled() {
                return Err("cancelled compiling instant transition programs".into());
            }
            let Some(source_wire) = wire_source_index(graph, wire.node) else {
                failure = Some("wire_source");
                break;
            };
            let Some(Some(query)) = drives.by_wire.get(source_wire).copied() else {
                failure = Some("missing_typed_drive");
                break;
            };
            wire_queries.push(query);

            // Every retained callback slot which targets a member wire must
            // select admission from a statically known source-wire bit. This
            // includes slots which do not contribute electrical power.
            for (slot, &site) in wire.neighbors.iter().enumerate() {
                if site == u32::MAX || site as usize >= net.site_wire.len() {
                    continue;
                }
                let target_wire = net.site_wire[site as usize];
                if target_wire == u32::MAX {
                    continue;
                }
                if target_wire as usize >= net.wires.len() {
                    failure = Some("invalid_member_target");
                    break;
                }
                let Some(selection) = graph.wires[source_wire].execution.callback_selection else {
                    failure = Some("missing_callback_selection");
                    break;
                };
                if selection.known & (1 << slot) == 0 {
                    failure = Some("unknown_internal_callback_slot");
                    break;
                }
            }
            if failure.is_some() {
                break;
            }
        }
        if let Some(reason) = failure {
            decline(&mut result.stats, reason);
            continue;
        }
        bytes += row_bytes;
        result.stats.eligible_nets += 1;
        result.stats.eligible_wires += wire_queries.len();
        result.nets[net_index] = Some(InstantTransitionNet {
            wire_queries: wire_queries.into_boxed_slice(),
        });
    }

    result.stats.work = work;
    result.stats.bytes = bytes;
    Ok(result)
}

fn wire_source_index(graph: &CompileGraph, node: NodeId) -> Option<usize> {
    match &graph.nodes.get(node.index())?.kind {
        CircuitNodeKind::Wire(index) if *index < graph.wires.len() => Some(*index),
        _ => None,
    }
}

fn decline(stats: &mut InstantTransitionProgramStats, reason: &str) {
    stats.declined_nets += 1;
    *stats.declines.entry(reason.to_owned()).or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::{analysis::AnalysisLimits, passes};

    fn compiled_graph() -> CompileGraph {
        let (world, bounds) = passes::simplify_instant_resets::tests::fixture();
        let monitor = TaskMonitor::default();
        let mut prepared = passes::rich::prepare(&world, bounds, vec![], &monitor, AnalysisLimits::default()).unwrap();
        let options = crate::redpiler::CompilerOptions {
            optimization_level: Some(5),
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
        prepared.graph
    }

    #[test]
    fn instant_transition_program_compilation_honors_cancellation() {
        let graph = compiled_graph();
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(
            infer_instant_transition_programs(&graph, &graph.plans.instant_transport, &graph.plans.instant_drives, &monitor,)
                .unwrap_err()
                .contains("cancelled")
        );
    }

    #[test]
    fn plan_change_rejects_a_forged_transition_program() {
        let mut graph = compiled_graph();
        let net_index = graph
            .plans
            .instant_transition_programs
            .nets
            .iter()
            .position(Option::is_some)
            .expect("fixture has an eligible exact transport net");
        let program = graph.plans.instant_transition_programs.nets[net_index].as_mut().unwrap();
        assert!(!program.wire_queries.is_empty());
        program.wire_queries[0] ^= 1;
        assert!(
            graph
                .validate_plan_change(PlanChange::InstantTransitionPrograms, &TaskMonitor::default())
                .is_err()
        );
        assert!(graph.plans.instant_transition_programs.nets[net_index].is_some());
    }
}
