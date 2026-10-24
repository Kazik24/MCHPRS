use crate::redpiler::components::{NodeState, NodeType};
use crate::redpiler::program_graph::{CircuitNodeKind, CompileGraph, LayerDeclineReason, LayerInputRole, LayerWitness};
use crate::redpiler::TaskMonitor;

pub(super) fn report(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    if monitor.cancelled() {
        return Err("cancelled while reporting timing diagnostics".into());
    }
    let (mut lines, mut emitted) = scheduled_components(graph, monitor)?;
    let Some(plan) = &graph.plans.instant.layers else {
        lines.push("timing: causal layer metadata was not computed; no timing conclusion available".into());
        return Ok(lines);
    };
    let (mut sampled, mut events, mut acceptance, mut lifecycle, mut depth) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for region in &plan.regions {
        if monitor.cancelled() {
            return Err("cancelled while reporting timing diagnostics".into());
        }
        for input in &region.inputs {
            if monitor.cancelled() {
                return Err("cancelled while reporting timing diagnostics".into());
            }
            match input.role {
                LayerInputRole::RequestSample => sampled += 1,
                LayerInputRole::ActivationEvent => events += 1,
                LayerInputRole::AcceptanceRevalidation => acceptance += 1,
            }
        }
        lifecycle += region.cold_lifecycle_occurrences.len();
        depth = depth.max(region.layers.len());
    }

    lines.push(format!(
        "timing: {} compiled finite causal regions; maximum layer depth {}; input bindings: request-sample={sampled}, activation-event={events}, acceptance-revalidation={acceptance}; retained Graph lifecycle boundaries={lifecycle}",
        plan.regions.len(), depth
    ));
    lines.push("layer depth is causal ordering only; it does not imply ticks, nanoticks, or physical duration".into());

    for region in &plan.regions {
        if emitted == super::MAX_WITNESSES {
            break;
        }
        let actor = region.layers.iter().flat_map(|layer| &layer.uses).next().map(|usage| usage.actor);
        let position = actor.and_then(|actor| graph.pistons.get(actor.index()).map(|piston| piston.position));
        lines.push(format!(
            "region {}{}: cells={}, corridors={}, causal layers={}, uses={}",
            region.id.0,
            position.map(|pos| format!(" at ({}, {}, {})", pos.x, pos.y, pos.z)).unwrap_or_default(),
            region.cells.len(),
            region.corridors.len(),
            region.layers.len(),
            region.layers.iter().map(|layer| layer.uses.len()).sum::<usize>()
        ));
        emitted += 1;
    }
    for decline in &plan.declines {
        if emitted == super::MAX_WITNESSES {
            break;
        }
        let pos = witness_position(graph, decline.witness);
        lines.push(format!(
            "unproven region witness{}: {} ({} cells)",
            pos.map(|p| format!(" at ({}, {}, {})", p.x, p.y, p.z)).unwrap_or_default(),
            decline_reason(decline.reason),
            decline.cells.len()
        ));
        emitted += 1;
    }
    let total = plan.regions.len() + plan.declines.len();
    if emitted < total {
        lines.push(format!("{} additional region witnesses omitted", total - emitted));
    }
    if monitor.cancelled() {
        return Err("cancelled while reporting timing diagnostics".into());
    }
    Ok(lines)
}

fn scheduled_components(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<(Vec<String>, usize), String> {
    let (mut repeaters, mut max_delay, mut max_repeater_pos, mut first_observer, mut observers, mut pending) =
        (0usize, 0u8, None, None, 0usize, 0usize);
    for node in &graph.nodes {
        if monitor.cancelled() {
            return Err("cancelled while reporting timing diagnostics".into());
        }
        match &node.kind {
            CircuitNodeKind::Electrical(NodeType::Repeater { delay, .. }) => {
                repeaters += 1;
                if *delay > max_delay {
                    max_delay = *delay;
                    max_repeater_pos = node.position;
                } else if *delay == max_delay && max_repeater_pos.is_none() {
                    max_repeater_pos = node.position;
                }
            }
            CircuitNodeKind::Electrical(NodeType::Observer { watched }) => {
                observers += 1;
                if first_observer.is_none() {
                    first_observer = node.position.map(|position| (position, *watched));
                }
            }
            CircuitNodeKind::Observer { watched, .. } | CircuitNodeKind::BuiltinReset { watched, .. } => {
                observers += 1;
                if first_observer.is_none() {
                    first_observer = graph
                        .sites
                        .get(watched.index())
                        .map(|site| (node.position.unwrap_or(site.position), site.position));
                }
            }
            _ => {}
        }
        if matches!(
            &node.state,
            crate::redpiler::program_graph::ComponentState::Electrical(NodeState { pending_tick: true, .. })
        ) {
            pending += 1;
        }
    }
    let mut lines = vec![format!("timing-capable component inventory: repeaters={repeaters}, observer/reset nodes={observers}; components marked with pending tick state={pending}")];
    let mut details = Vec::new();
    if let Some(position) = max_repeater_pos {
        details.push(format!("longest-delay repeater at ({}, {}, {}): configured delay {} redstone ticks ({} game ticks); this is device delay, not end-to-end circuit latency", position.x, position.y, position.z, max_delay, u16::from(max_delay) * 2));
    }
    if let Some((position, watched)) = first_observer {
        details.push(format!(
            "observer at ({}, {}, {}) watches ({}, {}, {})",
            position.x, position.y, position.z, watched.x, watched.y, watched.z
        ));
    }
    let emitted = details.len().min(super::MAX_WITNESSES);
    lines.extend(details.into_iter().take(emitted));
    Ok((lines, emitted))
}

fn witness_position(graph: &CompileGraph, witness: LayerWitness) -> Option<mchprs_blocks::BlockPos> {
    let node = match witness {
        LayerWitness::Actor(actor) | LayerWitness::PowerAtCut { actor, .. } => {
            return graph.pistons.get(actor.index()).map(|piston| piston.position);
        }
        LayerWitness::Wire(node) => node,
        LayerWitness::Callback(occurrence) => graph.routes.get(occurrence.route.index())?.source,
        LayerWitness::Power(link) => match link.to {
            crate::redpiler::program_graph::PowerTarget::Input { node, .. } => node,
            crate::redpiler::program_graph::PowerTarget::Conductor { site, .. } => {
                return graph.sites.get(site.index()).map(|site| site.position);
            }
        },
    };
    graph.nodes.get(node.index()).and_then(|node| node.position)
}

fn decline_reason(reason: LayerDeclineReason) -> &'static str {
    match reason {
        LayerDeclineReason::ProofBudgetExceeded => "analysis work or storage budget was exceeded",
        LayerDeclineReason::SignalArrivalUnproven => "electrical arrival at a request or acceptance boundary is unproven",
        LayerDeclineReason::CallbackSemanticsUnsupported => "callback capture or receiver semantics are unsupported",
        LayerDeclineReason::SharedOwnerOrderUnknown => "shared-owner request order is unknown",
        LayerDeclineReason::BoundaryEffectUnsupported => "an external analog or geometry effect is unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::{PlotWorld, PLOT_WIDTH};
    use crate::redpiler::analysis::graph::prepare_candidate_graph;
    use crate::redpiler::program_graph::*;
    use crate::redpiler::CompilerOptions;
    use crate::world::storage::Chunk;
    use crate::world::World;
    use mchprs_blocks::blocks::{Block, RedstoneRepeater};
    use mchprs_blocks::BlockPos;

    fn empty_graph() -> CompileGraph {
        CircuitDraft {
            nodes: vec![],
            links: vec![],
            power_links: vec![],
            comparator_inputs: vec![],
            profiles: vec![],
            guards: vec![],
            contacts: vec![],
            routes: vec![],
            wires: vec![],
            wire_relations: vec![],
            sites: vec![],
            pistons: vec![],
            resources: vec![],
            initial_pistons: vec![],
            initial_resources: vec![],
            plans: Default::default(),
        }
    }

    #[test]
    fn absent_layers_are_reported_as_not_computed() {
        let mut graph = empty_graph();
        graph.nodes.push(CircuitNodeDraft {
            position: None,
            raw_state: None,
            kind: CircuitNodeKind::Electrical(NodeType::Repeater {
                delay: 4,
                facing_diode: false,
            }),
            state: ComponentState::Electrical(NodeState {
                pending_tick: true,
                ..Default::default()
            }),
            output_route: None,
        });
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        assert!(lines.iter().any(|line| line.contains("not computed")));
        assert!(lines
            .iter()
            .any(|line| line.contains("repeaters=1") && line.contains("pending tick state=1")));
    }

    #[test]
    fn cancellation_stops_the_report() {
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(report(&empty_graph(), &monitor).unwrap_err().contains("cancelled"));
    }

    #[test]
    fn layer_roles_depth_and_decline_position_are_reported() {
        let mut graph = empty_graph();
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(4, 5, 6)),
            raw_state: None,
            kind: CircuitNodeKind::Wire(0),
            state: ComponentState::Specialized,
            output_route: None,
        });
        graph.plans.instant.layers = Some(LayeredInstantPlan {
            regions: vec![LayeredRegion {
                id: LayerRegionId(1),
                cells: vec![].into(),
                corridors: vec![].into(),
                stationary_entry: LayerExprId(0),
                inputs: [
                    LayerInputRole::RequestSample,
                    LayerInputRole::ActivationEvent,
                    LayerInputRole::AcceptanceRevalidation,
                ]
                .into_iter()
                .map(|role| LayerInput {
                    source: LayerInputSource::State(StateTest::PistonExtended(PistonId::new(0))),
                    role,
                })
                .collect(),
                outputs: vec![].into(),
                graph_outputs: vec![].into(),
                resource_outputs: vec![].into(),
                cold_lifecycle_occurrences: vec![].into(),
                expressions: vec![].into(),
                pin_values: vec![].into(),
                layers: vec![CausalLayer {
                    order: LayerOrder::Fifo,
                    uses: vec![].into(),
                }]
                .into(),
            }]
            .into(),
            programs: vec![].into(),
            declines: (0..9)
                .map(|_| LayerDecline {
                    cells: vec![0].into(),
                    corridors: vec![].into(),
                    witness: LayerWitness::Wire(NodeId::new(0)),
                    reason: LayerDeclineReason::SignalArrivalUnproven,
                })
                .collect(),
        });
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        let output = lines.join("\n");
        assert!(output.contains("request-sample=1, activation-event=1, acceptance-revalidation=1"));
        assert!(output.contains("maximum layer depth 1"));
        assert!(output.contains("at (4, 5, 6)"));
        assert!(output.contains("electrical arrival at a request or acceptance boundary is unproven"));
        assert_eq!(lines.iter().filter(|line| line.starts_with("unproven region witness")).count(), 7);
        assert!(output.contains("2 additional region witnesses omitted"));
    }

    #[test]
    fn o0_prepared_components_report_redstone_delay_and_watched_position() {
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        let repeater = BlockPos::new(40, 30, 40);
        let observer = BlockPos::new(42, 30, 40);
        world.set_block(
            repeater,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater::default(),
            },
        );
        world.set_block(
            observer,
            Block::Observer {
                observer: Default::default(),
            },
        );
        let monitor = std::sync::Arc::new(TaskMonitor::default());
        let candidate = prepare_candidate_graph(&world, (repeater, observer), &[], &CompilerOptions::default(), monitor).unwrap();
        let lines = report(&candidate.graph, &TaskMonitor::default()).unwrap();
        let timing = lines.join("\n");
        let delay = candidate
            .graph
            .nodes
            .iter()
            .find_map(|node| match &node.kind {
                CircuitNodeKind::Electrical(NodeType::Repeater { delay, .. }) => Some(*delay),
                _ => None,
            })
            .unwrap();
        assert!(timing.contains("repeaters=1"));
        assert!(timing.contains(&format!("configured delay {delay} redstone ticks ({} game ticks)", u16::from(delay) * 2)));
        assert!(timing.contains("longest-delay repeater at (40, 30, 40)"));
        assert!(timing.contains("observer at (42, 30, 40) watches ("));
    }
}
