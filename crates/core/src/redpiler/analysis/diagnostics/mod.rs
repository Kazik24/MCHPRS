//! Structural inspection of a prepared physical graph.
use crate::redpiler::TaskMonitor;
use crate::redpiler::program_graph::{CompileGraph, InputPort, LinkKind, PowerTarget};
use mchprs_blocks::BlockPos;
use rustc_hash::FxHashSet;

pub(crate) fn summary(graph: &CompileGraph) -> String {
    format!("Pistons: {}; execution uses the physical Graph runtime.", graph.pistons.len())
}

pub(crate) fn inspect(graph: &CompileGraph, position: BlockPos, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    if monitor.cancelled() {
        return Err("inspection cancelled".into());
    }
    let mut lines = vec![format!("Inspect ({}, {}, {})", position.x, position.y, position.z)];
    let node = graph.nodes.iter().enumerate().find(|(_, node)| node.position == Some(position));
    if let Some((index, node)) = node {
        lines.push(format!("Component: {:?}", node.kind));
        lines.extend(incoming_paths(graph, index, monitor)?);
        if let Some((actor_index, piston)) = graph.pistons.iter().enumerate().find(|(_, piston)| piston.node.index() == index) {
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
    } else {
        lines.push("No compiled component at this position.".into());
    }
    lines.push("Structural inspection only; it may miss paths and does not prove a circuit fault.".into());
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
        if let PowerTarget::Input { node, port } = link.to
            && node.index() == index
        {
            if port == InputPort::Quasi {
                quasi += 1;
            } else {
                power += 1;
            }
        }
    }
    lines.push(format!("Incoming power paths: {power}; quasi-power paths: {quasi}."));
    lines.push(format!(
        "Incoming callbacks: {updates} updates from {} routes; {shapes} shape checks. Counts include guarded alternatives.",
        routes.len()
    ));
    Ok(lines)
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
    fn inspection_counts_duplicate_deliveries_and_stays_local() {
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
        assert_eq!(before[1].split(" updates from ").nth(1), after[1].split(" updates from ").nth(1));
        let lines = inspect(&graph, position, &TaskMonitor::default()).unwrap();
        assert!(lines.len() <= 8);
        assert!(!lines.iter().any(|line| line.contains("(10, 20, 5)")));
        assert!(lines.last().unwrap().contains("Structural inspection only"));
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
}
