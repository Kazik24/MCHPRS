//! Targeted structural inspection; dependency counts are not timing warnings.
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::{CellDeclineReason, CompileGraph, InputPort, LinkKind, PowerTarget};
use mchprs_blocks::BlockPos;
use rustc_hash::FxHashSet;

pub(crate) fn summary(graph: &CompileGraph) -> String {
    let Some(catalog) = &graph.plans.instant.boundaries else {
        return format!("Pistons: {}; local instant evidence not analyzed at this tier.", graph.pistons.len());
    };
    let admitted: FxHashSet<_> = catalog.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect();
    format!(
        "Pistons: {}; local instant evidence {}; remaining {}. Local evidence is not a complete lifecycle proof.",
        graph.pistons.len(),
        admitted.len(),
        graph.pistons.len().saturating_sub(admitted.len())
    )
}

pub(crate) fn inspect(graph: &CompileGraph, position: BlockPos, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    if monitor.cancelled() {
        return Err("inspection cancelled".into());
    }
    let mut lines = vec![format!("Inspect ({}, {}, {})", position.x, position.y, position.z)];
    let Some((index, node)) = graph.nodes.iter().enumerate().find(|(_, node)| node.position == Some(position)) else {
        lines.push("No compiled component at this position.".into());
        return Ok(lines);
    };
    lines.push(format!("Component: {:?}", node.kind));
    lines.extend(incoming_paths(graph, index, monitor)?);
    if let Some((actor_index, piston)) = graph.pistons.iter().enumerate().find(|(_, piston)| piston.node.index() == index) {
        let inferred;
        let (cells, declines) = match &graph.plans.instant.boundaries {
            Some(catalog) => (&catalog.cells[..], &catalog.declines[..]),
            None => {
                inferred = crate::redpiler::analysis::instant::cells::discover_cells(graph, monitor)?;
                (&inferred.cells[..], &inferred.declined[..])
            }
        };
        if let Some(cell) = cells
            .iter()
            .position(|cell| cell.uses.iter().any(|use_| use_.actor.index() == actor_index))
        {
            lines.push(format!(
                "Local instant evidence: pull cell {cell}. This is not a complete lifecycle proof."
            ));
        } else if let Some(decline) = declines
            .iter()
            .find(|decline| decline.actors.iter().any(|actor| actor.index() == actor_index))
        {
            lines.push(format!(
                "Local instant evidence unavailable: {}. This does not prove non-instant behavior.",
                decline_reason(decline.reason)
            ));
        } else {
            lines.push("Local instant evidence unavailable: no recognized pull cell. This does not prove non-instant behavior.".into());
        }
        let mut other_contacts = FxHashSet::default();
        for (i, owner) in graph.pistons.iter().enumerate() {
            if i & 0x3ff == 0 && monitor.cancelled() {
                return Err("inspection cancelled".into());
            }
            if i != actor_index {
                other_contacts.extend([owner.near, owner.far]);
            }
        }
        let mut shared = 0usize;
        for (i, resource) in graph.resources.iter().enumerate() {
            if i & 0x3ff == 0 && monitor.cancelled() {
                return Err("inspection cancelled".into());
            }
            if (resource.contacts.contains(&piston.near) || resource.contacts.contains(&piston.far))
                && resource.contacts.iter().any(|contact| other_contacts.contains(contact))
            {
                shared += 1;
            }
        }
        lines.push(format!("Shared payload resources: {shared}."));
    }
    lines.push("Structural inspection only; no timing or whole-circuit correctness proof.".into());
    if monitor.cancelled() {
        return Err("inspection cancelled".into());
    }
    Ok(lines)
}

fn incoming_paths(graph: &CompileGraph, index: usize, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    let mut updates = 0usize;
    let mut shapes = 0usize;
    let mut routes = FxHashSet::default();
    for (i, link) in graph.links.iter().enumerate() {
        if i & 0x3ff == 0 && monitor.cancelled() {
            return Err("inspection cancelled".into());
        }
        if link.to.index() != index {
            continue;
        }
        match link.kind {
            LinkKind::Update { .. } => {
                updates += 1;
                routes.insert(link.callback.route);
            }
            LinkKind::Shape { .. } => shapes += 1,
        }
    }
    let mut power = 0usize;
    let mut quasi = 0usize;
    for (i, link) in graph.power_links.iter().enumerate() {
        if i & 0x3ff == 0 && monitor.cancelled() {
            return Err("inspection cancelled".into());
        }
        if let PowerTarget::Input { node, port } = link.to {
            if node.index() == index {
                if port == InputPort::Quasi {
                    quasi += 1;
                } else {
                    power += 1;
                }
            }
        }
    }
    lines.push(format!("Incoming power paths: {power}; QC paths: {quasi}."));
    lines.push(format!(
        "Incoming callbacks: {updates} updates from {} routes; {shapes} shape checks. Counts include guarded alternatives.",
        routes.len()
    ));
    Ok(lines)
}

fn decline_reason(reason: CellDeclineReason) -> &'static str {
    match reason {
        CellDeclineReason::UnrecognizedActor(_) => "reset not recognized",
        CellDeclineReason::DifferentPullSources(_) => "different payload pull sources",
        CellDeclineReason::NoPullSource(_) => "no supported pull lane",
        CellDeclineReason::MissingDeferredWake(_) => "no restore route",
        CellDeclineReason::PinTableBudgetExceeded => "analysis budget exceeded",
        CellDeclineReason::ConflictingSourceContact(_) => "conflicting payload contacts",
        CellDeclineReason::UnsupportedMaterial(_) => "unsupported payload material",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::PlotWorld;
    use crate::redpiler::analysis::graph::prepare_candidate_graph;
    use crate::world::{World, storage::Chunk};
    use mchprs_blocks::blocks::Block;
    use std::sync::Arc;

    fn graph() -> CompileGraph {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        world.set_block(BlockPos::new(5, 20, 5), Block::from_name("piston").unwrap());
        world.set_block(BlockPos::new(10, 20, 5), Block::from_name("piston").unwrap());
        prepare_candidate_graph(
            &world,
            (BlockPos::new(0, 0, 0), BlockPos::new(15, 31, 15)),
            &[],
            &Default::default(),
            Arc::default(),
        )
        .unwrap()
        .graph
    }

    #[test]
    fn inspection_is_targeted_and_preserves_duplicate_deliveries() {
        let mut graph = graph();
        let position = BlockPos::new(5, 20, 5);
        let index = graph.nodes.iter().position(|node| node.position == Some(position)).unwrap();
        let update = graph
            .links
            .iter()
            .find(|link| link.to.index() == index && matches!(link.kind, LinkKind::Update { .. }))
            .unwrap()
            .clone();
        let before = incoming_paths(&graph, index, &TaskMonitor::default()).unwrap();
        graph.links.push(update);
        let after = incoming_paths(&graph, index, &TaskMonitor::default()).unwrap();
        assert_eq!(before[0], after[0]);
        assert_ne!(before[1], after[1], "duplicate callbacks count as deliveries");
        assert_eq!(
            before[1].split(" updates from ").nth(1),
            after[1].split(" updates from ").nth(1),
            "duplicating a delivery does not add a route"
        );
        let lines = inspect(&graph, position, &TaskMonitor::default()).unwrap();
        assert!(lines.len() <= 8);
        assert!(lines.iter().any(|line| line.contains("evidence")));
        assert!(!lines.iter().any(|line| line.contains("(10, 20, 5)")));
        assert_eq!(
            lines.last().unwrap(),
            "Structural inspection only; no timing or whole-circuit correctness proof."
        );
    }

    #[test]
    fn missing_component_and_cancelled_inspection_are_explicit() {
        let graph = graph();
        let position = BlockPos::new(0, 20, 0);
        assert_eq!(
            inspect(&graph, position, &TaskMonitor::default()).unwrap()[1],
            "No compiled component at this position."
        );
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert_eq!(inspect(&graph, position, &monitor).unwrap_err(), "inspection cancelled");
    }

    #[test]
    fn summary_does_not_claim_missing_analysis_is_a_failed_proof() {
        let mut graph = graph();
        assert_eq!(summary(&graph), "Pistons: 2; local instant evidence not analyzed at this tier.");
        graph.plans.instant.boundaries = Some(Default::default());
        let summary = summary(&graph);
        assert!(summary.contains("local instant evidence 0; remaining 2"));
        assert!(summary.contains("not a complete lifecycle proof"));
    }
}
