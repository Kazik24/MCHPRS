//! Targeted structural inspection; dependency counts are not timing warnings.
use crate::redpiler::TaskMonitor;
use crate::redpiler::components::NodeType;
use crate::redpiler::program_graph::{
    CellDeclineReason, CircuitNodeKind, CompileGraph, ComponentState, Guard, InputPort, LayerDecline, LayerDeclineReason, LayerWitness, LinkKind,
    NodeId, PistonId, PowerPort, PowerTarget, SiteSource,
};
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use rustc_hash::FxHashSet;

pub(crate) fn summary(graph: &CompileGraph) -> String {
    let Some(catalog) = &graph.plans.instant.boundaries else {
        return format!("Pistons: {}; instant cells not analyzed at this tier.", graph.pistons.len());
    };
    let admitted: FxHashSet<_> = catalog.cells.iter().flat_map(|cell| cell.uses.iter().map(|use_| use_.actor)).collect();
    format!(
        "Pistons: {}; recognized instant cells: {}; others: {}. Recognition does not prove circuit behavior.",
        graph.pistons.len(),
        admitted.len(),
        graph.pistons.len().saturating_sub(admitted.len())
    )
}

pub(crate) fn cell_decline_example(graph: &CompileGraph) -> Option<String> {
    // A singleton without a recognized reset is also synthesized for every ordinary piston.
    let decline = graph
        .plans
        .instant
        .boundaries
        .as_ref()?
        .declines
        .iter()
        .find(|decline| decline.actors.len() > 1 || !matches!(decline.reason, CellDeclineReason::UnrecognizedActor(_)))?;
    let actor = match decline.reason {
        CellDeclineReason::UnrecognizedActor(actor) | CellDeclineReason::MissingDeferredWake(actor) => actor,
        _ => *decline.actors.first()?,
    };
    let pos = graph.pistons.get(actor.index())?.position;
    if matches!(decline.reason, CellDeclineReason::UnrecognizedActor(_))
        && let Some(source) = fixed_redstone_input(graph, actor)
    {
        return Some(format!(
            "Instant optimization at ({}, {}, {}): reset not recognized. Redstone block at ({}, {}, {}) may intentionally hold this piston. An ordinary piston is valid; this is a compiler limitation.",
            pos.x, pos.y, pos.z, source.x, source.y, source.z
        ));
    }
    Some(format!(
        "Instant cell at ({}, {}, {}): {}. {} This does not prove a circuit fault.",
        pos.x,
        pos.y,
        pos.z,
        decline_reason(decline.reason),
        cell_decline_guidance(decline.reason)
    ))
}

fn fixed_redstone_input(graph: &CompileGraph, actor: PistonId) -> Option<BlockPos> {
    let node = graph.pistons.get(actor.index())?.node;
    graph.power_links.iter().find_map(|link| {
        if !matches!(link.to, PowerTarget::Input { node: target, port: InputPort::Main | InputPort::Quasi } if target == node)
            || !matches!(graph.guards.get(link.guard.index()), Some(Guard::Always))
            || link.attenuation >= 15
            || !matches!(link.from_port, PowerPort::Weak(_) | PowerPort::Output | PowerPort::Stored)
        {
            return None;
        }
        let site = graph.sites.get(link.from.index())?;
        let SiteSource::Component(source) = site.source else { return None };
        let source = graph.nodes.get(source.index())?;
        (matches!(source.kind, CircuitNodeKind::Electrical(NodeType::Constant))
            && matches!(&source.state, ComponentState::Electrical(state) if state.output_strength > link.attenuation)
            && source.raw_state.map(Block::from_id) == Some(Block::RedstoneBlock))
        .then_some(site.position)
    })
}

/// Show a connected candidate's actual witness without claiming that all other paths failed.
pub(crate) fn layer_decline_example(graph: &CompileGraph) -> Option<String> {
    let plan = graph.plans.instant.layers.as_ref()?;
    let decline = plan
        .declines
        .iter()
        .filter(|decline| decline.cells.len() > 1 && decline.reason != LayerDeclineReason::NoUsefulEventPath)
        .max_by_key(|decline| decline.cells.len())?;
    let (reason, source, target) = layer_decline_details(graph, decline)?;
    let position = |pos: BlockPos| format!("({}, {}, {})", pos.x, pos.y, pos.z);
    let source = source.or(target)?;
    let location = match target.filter(|target| *target != source) {
        Some(target) => format!("{} to {}", position(source), position(target)),
        None => position(source),
    };
    let inspect = if matches!(decline.witness, LayerWitness::Actor(_)) {
        " Reset, aim at the piston, and use /rp inspect."
    } else {
        ""
    };
    Some(format!(
        "Instant region skipped: {reason} at {location} ({} cell groups). {} This does not prove a circuit fault.{inspect}",
        decline.cells.len(),
        layer_decline_guidance(decline.reason)
    ))
}

fn layer_decline_details(graph: &CompileGraph, decline: &LayerDecline) -> Option<(&'static str, Option<BlockPos>, Option<BlockPos>)> {
    let reason = match decline.reason {
        LayerDeclineReason::NoUsefulEventPath => "no useful event path found",
        LayerDeclineReason::ProofBudgetExceeded => "analysis budget exceeded",
        LayerDeclineReason::SignalArrivalUnproven => "signal path or update not verified",
        LayerDeclineReason::CallbackSemanticsUnsupported => "update receiver or captured state not supported",
        LayerDeclineReason::SharedOwnerOrderUnknown => "shared payload request order not verified",
        LayerDeclineReason::BoundaryEffectUnsupported => "external effect not supported",
    };
    let actor_position = |actor: PistonId| graph.pistons.get(actor.index()).map(|piston| piston.position);
    let node_position = |node: NodeId| graph.nodes.get(node.index()).and_then(|node| node.position);
    let (source, target) = match decline.witness {
        LayerWitness::Actor(actor) => (actor_position(actor), None),
        LayerWitness::Wire(node) => (node_position(node), None),
        LayerWitness::Power(link) => (
            graph.sites.get(link.from.index()).map(|site| site.position),
            match link.to {
                PowerTarget::Input { node, .. } => node_position(node),
                PowerTarget::Conductor { site, .. } => graph.sites.get(site.index()).map(|site| site.position),
            },
        ),
        LayerWitness::PowerAtCut { actor, link, .. } => (graph.sites.get(link.from.index()).map(|site| site.position), actor_position(actor)),
        LayerWitness::Callback(occurrence) => {
            let route = graph.routes.get(occurrence.route.index())?;
            let target = route
                .deliveries
                .get(occurrence.ordinal as usize)
                .and_then(|delivery| graph.sites.get(delivery.target.index()))
                .map(|site| site.position);
            (node_position(route.source), target)
        }
    };
    Some((reason, source, target))
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
            let inferred;
            let (cells, declines) = match &graph.plans.instant.boundaries {
                Some(catalog) => (&catalog.cells[..], &catalog.declines[..]),
                None => {
                    inferred = crate::redpiler::analysis::instant::cells::discover_cells(graph, monitor)?;
                    (&inferred.cells[..], &inferred.declined[..])
                }
            };
            let cell = cells
                .iter()
                .position(|cell| cell.uses.iter().any(|use_| use_.actor.index() == actor_index));
            if let Some(cell) = cell {
                lines.push(format!("Recognized instant cell {cell}."));
            }
            let mut count = 0;
            for decline in declines
                .iter()
                .filter(|decline| decline.actors.iter().any(|actor| actor.index() == actor_index))
            {
                count += 1;
                if count <= 8 {
                    let witness = match decline.reason {
                        CellDeclineReason::UnrecognizedActor(actor) | CellDeclineReason::MissingDeferredWake(actor) => {
                            graph.pistons.get(actor.index()).map(|piston| piston.position)
                        }
                        CellDeclineReason::ConflictingSourceContact(contact) => graph.contacts.get(contact.index()).map(|contact| contact.position),
                        _ => None,
                    };
                    let witness = witness.map_or_else(String::new, |pos| format!(" at {pos:?}"));
                    lines.push(format!(
                        "Instant cell not recognized: {}{witness}. {}",
                        decline_reason(decline.reason),
                        cell_decline_guidance(decline.reason)
                    ));
                }
            }
            if cell.is_none() && count == 0 {
                lines.push("No instant cell recognized for this piston.".into());
            } else if count > 8 {
                lines.push(format!("{} additional local decline(s) omitted.", count - 8));
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
    } else {
        lines.push("No compiled component at this position.".into());
    }
    match (&graph.plans.instant.boundaries, &graph.plans.instant.layers) {
        (Some(catalog), Some(plan)) => {
            let mut count = 0;
            for decline in &plan.declines {
                if monitor.cancelled() {
                    return Err("inspection cancelled".into());
                }
                let Some((reason, source, target)) = layer_decline_details(graph, decline) else {
                    continue;
                };
                let member = decline.cells.iter().any(|&cell| {
                    catalog.cells.get(cell as usize).is_some_and(|cell| {
                        cell.uses.iter().any(|use_| {
                            graph
                                .pistons
                                .get(use_.actor.index())
                                .is_some_and(|actor| Some(actor.node.index()) == node.map(|(index, _)| index))
                        })
                    })
                }) || decline.corridors.iter().any(|&corridor| {
                    catalog
                        .corridors
                        .get(corridor as usize)
                        .is_some_and(|corridor| corridor.wires.iter().any(|wire| Some(wire.index()) == node.map(|(index, _)| index)))
                });
                if !member && source != Some(position) && target != Some(position) {
                    continue;
                }
                count += 1;
                if count <= 8 {
                    let location = match (source, target) {
                        (Some(source), Some(target)) if source != target => format!("; witness {source:?} to {target:?}"),
                        (Some(source), _) | (_, Some(source)) => format!("; witness {source:?}"),
                        _ => String::new(),
                    };
                    lines.push(format!(
                        "Connected instant candidate skipped: {reason}{location}. {}",
                        layer_decline_guidance(decline.reason)
                    ));
                }
            }
            if count > 8 {
                lines.push(format!("{} additional connected candidate decline(s) omitted.", count - 8));
            }
        }
        _ => lines.push("Connected instant candidates were not analyzed at this tier.".into()),
    }
    lines.push("Structural inspection only; it may miss declines and does not prove a circuit fault.".into());
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

fn cell_decline_guidance(reason: CellDeclineReason) -> &'static str {
    match reason {
        CellDeclineReason::UnrecognizedActor(_) => "This can be an ordinary piston; the compiler did not recognize an instant reset.",
        CellDeclineReason::MissingDeferredWake(_) => "The compiler could not verify the reset update path.",
        CellDeclineReason::DifferentPullSources(_) | CellDeclineReason::NoPullSource(_) => "Inspect the shared payload and its piston pull paths.",
        CellDeclineReason::ConflictingSourceContact(_) => "Inspect the payload positions and their piston owners.",
        CellDeclineReason::UnsupportedMaterial(_) => "Inspect the payload material and piston movement path.",
        CellDeclineReason::PinTableBudgetExceeded => "Try a lower optimization tier if compilation is slow or fails.",
    }
}

fn layer_decline_guidance(reason: LayerDeclineReason) -> &'static str {
    match reason {
        LayerDeclineReason::NoUsefulEventPath => "Inspect the incoming update paths if this region should be active.",
        LayerDeclineReason::ProofBudgetExceeded => "Try a lower optimization tier if compilation is slow or fails.",
        LayerDeclineReason::SignalArrivalUnproven => "Inspect the signal path and its update routes.",
        LayerDeclineReason::CallbackSemanticsUnsupported => "Inspect the update receiver and any captured neighboring state.",
        LayerDeclineReason::SharedOwnerOrderUnknown => "Inspect the shared payload and its piston update paths.",
        LayerDeclineReason::BoundaryEffectUnsupported => "Inspect the external consumer and the update path at the boundary.",
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
        assert!(lines.iter().any(|line| line.to_ascii_lowercase().contains("instant cell")));
        assert!(!lines.iter().any(|line| line.contains("(10, 20, 5)")));
        assert_eq!(
            lines.last().unwrap(),
            "Structural inspection only; it may miss declines and does not prove a circuit fault."
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
        assert_eq!(summary(&graph), "Pistons: 2; instant cells not analyzed at this tier.");
        graph.plans.instant.boundaries = Some(Default::default());
        let summary = summary(&graph);
        assert!(summary.contains("recognized instant cells: 0; others: 2"));
        assert!(summary.contains("does not prove circuit behavior"));
    }

    #[test]
    fn cell_decline_example_is_local_and_links_a_real_witness() {
        use crate::redpiler::components::NodeState;
        use crate::redpiler::program_graph::{CircuitNodeDraft, ComponentState, GuardId, NodeId, PowerLink, SiteId, SiteSpec};
        use mchprs_blocks::BlockFace;
        let mut graph = graph();
        assert!(cell_decline_example(&graph).is_none());
        graph.plans.instant.boundaries =
            Some(crate::redpiler::analysis::instant::boundaries::infer_boundaries(&graph, &TaskMonitor::default()).unwrap());
        assert!(
            cell_decline_example(&graph).is_none(),
            "ordinary unrecognized pistons are not candidate warnings"
        );
        let catalog = graph.plans.instant.boundaries.as_mut().unwrap();
        let actors = catalog
            .declines
            .iter()
            .flat_map(|decline| decline.actors.iter().copied())
            .collect::<Vec<_>>();
        let witness = actors[1];
        catalog.declines = vec![crate::redpiler::program_graph::CellDecline {
            reason: CellDeclineReason::UnrecognizedActor(witness),
            actors,
        }]
        .into_boxed_slice();
        let message = cell_decline_example(&graph).unwrap();
        assert!(message.contains("reset not recognized"));
        assert!(message.contains("This can be an ordinary piston"));
        assert!(message.contains("does not prove a circuit fault"));
        let component = crate::player::diagnostic_component(&message);
        let position = graph.pistons[witness.index()].position;
        let expected = format!("/hl {} {} {}", position.x, position.y, position.z);
        assert!(
            component["extra"]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part["click_event"]["command"] == expected)
        );

        let source_position = position.offset(BlockFace::Top).offset(BlockFace::Top);
        let source_node = NodeId::new(graph.nodes.len() as u32);
        graph.nodes.push(CircuitNodeDraft {
            position: Some(source_position),
            raw_state: Some(Block::RedstoneBlock.get_id()),
            kind: CircuitNodeKind::Electrical(NodeType::Constant),
            state: ComponentState::Electrical(NodeState::with_strength(15)),
            output_route: None,
        });
        let source_site = SiteId::new(graph.sites.len() as u32);
        graph.sites.push(SiteSpec {
            position: source_position,
            source: SiteSource::Component(source_node),
            source_strength: 15,
        });
        let guard = GuardId::new(graph.guards.len() as u32);
        graph.guards.push(Guard::Always);
        graph.power_links.push(PowerLink {
            from: source_site,
            from_port: PowerPort::Weak(BlockFace::Top),
            to: PowerTarget::Input {
                node: graph.pistons[witness.index()].node,
                port: InputPort::Quasi,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard,
        });
        let message = cell_decline_example(&graph).unwrap();
        assert!(message.contains("may intentionally hold this piston"));
        assert!(message.contains("ordinary piston is valid"));
        let source_command = format!("/hl {} {} {}", source_position.x, source_position.y, source_position.z);
        let component = crate::player::diagnostic_component(&message);
        assert!(
            component["extra"]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part["click_event"]["command"] == source_command)
        );
        graph.power_links.last_mut().unwrap().from_port = PowerPort::Strong(BlockFace::Top);
        assert!(
            !cell_decline_example(&graph).unwrap().contains("intentionally hold"),
            "redstone blocks do not emit strong power"
        );
        graph.power_links.last_mut().unwrap().from_port = PowerPort::Weak(BlockFace::Top);
        let ComponentState::Electrical(state) = &mut graph.nodes[source_node.index()].state else {
            unreachable!()
        };
        state.output_strength = 0;
        assert!(
            !cell_decline_example(&graph).unwrap().contains("intentionally hold"),
            "a zero source strength is not proof of power"
        );
    }

    #[test]
    fn connected_region_warning_links_a_real_witness_and_describes_only_that_candidate() {
        use crate::redpiler::program_graph::{LayerDecline, LayeredInstantPlan, PistonId};
        let mut graph = graph();
        assert!(layer_decline_example(&graph).is_none());
        let decline = |cells: usize, reason, actor| LayerDecline {
            cells: (0..cells as u32).collect(),
            corridors: Box::new([]),
            witness: LayerWitness::Actor(PistonId::new(actor)),
            reason,
        };
        graph.plans.instant.layers = Some(LayeredInstantPlan {
            declines: vec![
                decline(5, LayerDeclineReason::NoUsefulEventPath, 0),
                decline(2, LayerDeclineReason::SignalArrivalUnproven, 1),
                decline(1, LayerDeclineReason::ProofBudgetExceeded, 0),
            ]
            .into_boxed_slice(),
            ..Default::default()
        });
        let message = layer_decline_example(&graph).unwrap();
        assert!(message.contains("2 cell groups"));
        assert!(message.contains("signal path or update not verified"));
        assert!(message.contains("Inspect the signal path and its update routes"));
        assert!(message.contains("Instant region skipped"));
        assert!(message.contains("does not prove a circuit fault"));
        assert!(message.contains("use /rp inspect"));
        let position = graph.pistons[1].position;
        let command = format!("/hl {} {} {}", position.x, position.y, position.z);
        let component = crate::player::diagnostic_component(&message);
        assert!(
            component["extra"]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part["click_event"]["command"] == command)
        );
    }

    #[test]
    fn inspection_reports_matching_connected_declines_with_bounded_clickable_witnesses() {
        use crate::redpiler::program_graph::{GuardId, LayerDecline, LayeredInstantPlan, PowerLink, SiteId, SiteSpec};
        use mchprs_blocks::BlockFace;
        let mut graph = graph();
        graph.plans.instant.boundaries = Some(Default::default());
        let actor = PistonId::new(0);
        let position = graph.pistons[actor.index()].position;
        let decline = LayerDecline {
            cells: Box::new([]),
            corridors: Box::new([]),
            witness: LayerWitness::Actor(actor),
            reason: LayerDeclineReason::BoundaryEffectUnsupported,
        };
        graph.plans.instant.layers = Some(LayeredInstantPlan {
            declines: vec![decline; 10].into_boxed_slice(),
            ..Default::default()
        });
        let lines = inspect(&graph, position, &TaskMonitor::default()).unwrap();
        let matching = lines
            .iter()
            .filter(|line| line.contains("Connected instant candidate skipped:"))
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 8);
        assert!(matching[0].contains("external effect not supported"));
        assert!(matching[0].contains("Inspect the external consumer and the update path at the boundary"));
        assert!(lines.last().unwrap().contains("does not prove a circuit fault"));
        let command = format!("/hl {} {} {}", position.x, position.y, position.z);
        assert!(
            crate::player::diagnostic_component(matching[0])["extra"]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part["click_event"]["command"] == command)
        );
        assert!(lines.iter().any(|line| line == "2 additional connected candidate decline(s) omitted."));
        let other = graph.pistons[1].position;
        assert!(
            !inspect(&graph, other, &TaskMonitor::default())
                .unwrap()
                .iter()
                .any(|line| line.contains("Connected instant candidate skipped:"))
        );

        let source = SiteId::new(graph.sites.len() as u32);
        graph.sites.push(SiteSpec {
            position: other,
            source: SiteSource::Component(graph.pistons[1].node),
            source_strength: 0,
        });
        let link = PowerLink {
            from: source,
            from_port: PowerPort::Weak(BlockFace::Top),
            to: PowerTarget::Input {
                node: graph.pistons[0].node,
                port: InputPort::Main,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: GuardId::new(0),
        };
        graph.plans.instant.layers.as_mut().unwrap().declines = vec![LayerDecline {
            cells: Box::new([]),
            corridors: Box::new([]),
            witness: LayerWitness::Power(link),
            reason: LayerDeclineReason::SignalArrivalUnproven,
        }]
        .into_boxed_slice();
        let lines = inspect(&graph, position, &TaskMonitor::default()).unwrap();
        let message = lines.iter().find(|line| line.contains("Connected instant candidate skipped:")).unwrap();
        let component = crate::player::diagnostic_component(message);
        let parts = component["extra"].as_array().unwrap();
        let other_command = format!("/hl {} {} {}", other.x, other.y, other.z);
        assert!(parts.iter().any(|part| part["click_event"]["command"] == command));
        assert!(parts.iter().any(|part| part["click_event"]["command"] == other_command));

        let (site, site_position) = graph
            .sites
            .iter()
            .enumerate()
            .find(|(_, site)| !graph.nodes.iter().any(|node| node.position == Some(site.position)))
            .map(|(index, site)| (SiteId::new(index as u32), site.position))
            .expect("fixture has a query site without a compiled component");
        let decline = &mut graph.plans.instant.layers.as_mut().unwrap().declines[0];
        let LayerWitness::Power(ref mut link) = decline.witness else {
            unreachable!()
        };
        link.from = site;
        let lines = inspect(&graph, site_position, &TaskMonitor::default()).unwrap();
        assert!(lines.iter().any(|line| line == "No compiled component at this position."));
        let message = lines.iter().find(|line| line.contains("Connected instant candidate skipped:")).unwrap();
        let component = crate::player::diagnostic_component(message);
        let command = format!("/hl {} {} {}", site_position.x, site_position.y, site_position.z);
        assert!(
            component["extra"]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part["click_event"]["command"] == command)
        );
    }
}
