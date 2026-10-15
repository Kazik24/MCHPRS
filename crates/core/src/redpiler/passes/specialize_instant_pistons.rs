//! Recognizes observer-reset instant mechanisms, including shared and vertical actors.
//!
//! An observer watching a piston base supplies a concrete reset-power witness to
//! the actor's QC channel, directly or through a fixed conductor. The watched
//! actor and reset actor need not be the same; payload ownership remains shared.
//! This recognizes the full ADDER1BIT family, not just a private two-wire layout.
//!
//! The executable rewrite only expands fixed conductor reads into their exact
//! contributors: max distributes over guarded, saturating attenuation. All
//! contributors, Main/QC ports, live source reads and guards are retained.
//! No callback, request, observer, resource or lifecycle is absorbed. Thus the
//! rewrite is valid under arbitrary histories, not only prepared arithmetic.
//! Reset provenance is not a certificate for removing the surrounding mechanism.
//!
//! Discovery and rewriting index incident power links once. Publication is
//! atomic after cancellation checks; already expanded inputs are unchanged.
//! Unknown reset paths or recursive conductor inputs stay general. The separate
//! private output kernel keeps its own stronger proof and is not broadened here.

#[cfg(test)]
use super::utils::instant_mechanisms::discover_mechanisms;
use super::utils::instant_mechanisms::{conductor_inputs, discover_indexed_mechanisms, index_power};
use super::{GraphPass, PassContext};
use crate::redpiler::program_graph::{CompileGraph, Guard, GuardId, InputPort, PowerLink, PowerTarget};
use crate::redpiler::{PreparationTableStatistics, TaskMonitor};
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) struct SpecializeInstantPistons;

impl GraphPass for SpecializeInstantPistons {
    fn name(&self) -> &'static str {
        "specialize-instant-pistons"
    }

    fn run(&self, graph: &mut CompileGraph, context: &PassContext<'_>) -> Result<(), String> {
        let incoming = index_power(graph);
        let mechanisms = discover_indexed_mechanisms(graph, &incoming, context.monitor)?;
        let actors: FxHashSet<_> = mechanisms.iter().map(|entry| graph.pistons[entry.piston.index()].node).collect();
        let mut links = Vec::with_capacity(graph.power_links.len());
        let mut guards = Vec::new();
        let mut conjunctions = FxHashMap::default();
        let mut specialized = FxHashSet::default();
        // Shared conductors must not cause a Cartesian explosion of retained edges.
        let mut growth_budget = graph.power_links.len();
        for &link in &graph.power_links {
            check_cancelled(context.monitor)?;
            let PowerTarget::Input {
                node,
                port: InputPort::Main | InputPort::Quasi,
            } = link.to
            else {
                links.push(link);
                continue;
            };
            let contributors = actors.contains(&node).then(|| conductor_inputs(graph, &incoming, link)).flatten();
            let Some(contributors) = contributors else {
                links.push(link);
                continue;
            };
            let growth = contributors.len().saturating_sub(1);
            if growth > growth_budget {
                links.push(link);
                continue;
            }
            growth_budget -= growth;
            for contributor in contributors {
                let guard = join_guards(graph, link.guard, contributor.guard, &mut guards, &mut conjunctions)?;
                links.push(PowerLink {
                    from: contributor.from,
                    from_port: contributor.from_port,
                    to: link.to,
                    face: link.face,
                    attenuation: link.attenuation.saturating_add(contributor.attenuation).min(15),
                    guard,
                });
            }
            specialized.insert(node);
        }
        check_cancelled(context.monitor)?;
        graph.guards.extend(guards);
        graph.power_links = links;
        context.monitor.record_preparation_tables(
            [
                ("recognized instant actors", mechanisms.len()),
                ("specialized instant actors", specialized.len()),
                (
                    "shared-reset actors",
                    mechanisms.iter().filter(|entry| entry.piston != entry.watched_actor).count(),
                ),
            ]
            .into_iter()
            .map(|(table, entries)| PreparationTableStatistics {
                stage: "instant mechanism specialization",
                table: table.into(),
                entries,
                capacity: entries,
                estimated_bytes: 0,
            })
            .collect(),
        );
        Ok(())
    }

    fn preserves_wire_hops(&self) -> bool {
        true
    }

    fn preserves_dust_paths(&self) -> bool {
        true
    }
    fn preserves_turbo_callback_indices(&self) -> bool {
        true
    }
    fn preserves_wire_query_facts(&self) -> bool {
        true
    }
    fn preserves_instant_resets(&self) -> bool {
        true
    }
}

fn join_guards(
    graph: &CompileGraph,
    a: GuardId,
    b: GuardId,
    added: &mut Vec<Guard>,
    cache: &mut FxHashMap<(GuardId, GuardId), GuardId>,
) -> Result<GuardId, String> {
    if a == b || matches!(graph.guards[b.index()], Guard::Always) {
        return Ok(a);
    }
    if matches!(graph.guards[a.index()], Guard::Always) {
        return Ok(b);
    }
    let key = if a < b { (a, b) } else { (b, a) };
    if let Some(&id) = cache.get(&key) {
        return Ok(id);
    }
    let id = GuardId::try_from_index(graph.guards.len() + added.len()).ok_or("too many instant power guards")?;
    // Guard reads are pure; canonical pairs share a single equivalent conjunction.
    added.push(Guard::All(vec![key.0, key.1].into_boxed_slice()));
    cache.insert(key, id);
    Ok(id)
}

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while recognizing instant mechanisms".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "specialize_instant_pistons_tests.rs"]
mod tests;
