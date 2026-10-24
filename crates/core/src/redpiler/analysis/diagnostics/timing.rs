use crate::redpiler::program_graph::{CompileGraph, DeliveryKind, Guard, LayerDeclineReason, LayerInputRole, LayerWitness, OperandView};
use crate::redpiler::TaskMonitor;
use mchprs_blocks::BlockPos;

const MAX_ROUTE_WORK: usize = 100_000;
const ROUTE_STEPS: usize = 2;

pub(super) fn report(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    if monitor.cancelled() {
        return Err("cancelled while reporting timing diagnostics".into());
    }
    let (mut lines, route_details) = route_order(graph, monitor)?;

    let Some(plan) = &graph.plans.instant.layers else {
        lines.push("Timing: no FIFO proof; route order is local, not absolute nanoticks".into());
        lines.extend(route_details.into_iter().take(super::MAX_WITNESSES));
        check_cancelled(monitor)?;
        return Ok(lines);
    };

    let (mut samples, mut events, mut acceptance) = (0usize, 0usize, 0usize);
    for region in &plan.regions {
        check_cancelled(monitor)?;
        for input in &region.inputs {
            check_cancelled(monitor)?;
            match input.role {
                LayerInputRole::RequestSample => samples += 1,
                LayerInputRole::ActivationEvent => events += 1,
                LayerInputRole::AcceptanceRevalidation => acceptance += 1,
            }
        }
    }
    lines.push(format!(
        "Timing: {} regions; samples={samples}, events={events}, rechecks={acceptance}",
        plan.regions.len()
    ));
    let mut details = 0;
    for region in &plan.regions {
        check_cancelled(monitor)?;
        for (stage, layer) in region.layers.iter().enumerate() {
            check_cancelled(monitor)?;
            if layer.uses.is_empty() || details == super::MAX_WITNESSES {
                continue;
            }
            let mut steps = Vec::new();
            for (index, usage) in layer.uses.iter().enumerate() {
                check_cancelled(monitor)?;
                if index == ROUTE_STEPS {
                    break;
                }
                if let Some(position) = piston_position(graph, usage.actor) {
                    steps.push(format!("#{index} ({}, {}, {})", position.x, position.y, position.z));
                }
            }
            if !steps.is_empty() {
                let more = layer.uses.len().saturating_sub(steps.len());
                lines.push(format!(
                    "Timing: region {} FIFO stage {}: {}{}",
                    region.id.0,
                    stage + 1,
                    steps.join(" -> "),
                    if more == 0 { String::new() } else { format!(" +{more}") }
                ));
                details += 1;
            }
        }
    }
    if details == 0 && plan.regions.is_empty() {
        lines.push("Timing: no compiled finite causal layers".into());
    }

    for decline in &plan.declines {
        check_cancelled(monitor)?;
        if details == super::MAX_WITNESSES {
            break;
        }
        if let Some(position) = witness_position(graph, decline.witness) {
            lines.push(format!(
                "Timing: unknown at ({}, {}, {}): {}",
                position.x,
                position.y,
                position.z,
                decline_reason(decline.reason)
            ));
            details += 1;
        }
    }
    for line in route_details.into_iter().take(super::MAX_WITNESSES.saturating_sub(details)) {
        lines.push(line);
        details += 1;
    }
    if plan.regions.is_empty() && plan.declines.is_empty() && details == 0 {
        lines.push("Timing: no ordered event or causal-layer witness found".into());
    }
    lines.push("Timing: FIFO stages are conditional; route order is local, not absolute nanoticks".into());
    check_cancelled(monitor)?;
    Ok(lines)
}

fn route_order(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<(Vec<String>, Vec<String>), String> {
    let mut scanned = 0usize;
    let mut delivered = 0usize;
    let mut details = Vec::new();
    let mut truncated = false;
    'routes: for (route_id, route) in graph.routes.iter().enumerate() {
        check_cancelled(monitor)?;
        if route_id == MAX_ROUTE_WORK {
            truncated = true;
            break;
        }
        let Some(source) = graph.nodes.get(route.source.index()).and_then(|node| node.position) else {
            continue;
        };
        let mut steps = Vec::new();
        let mut relevant = 0usize;
        for (ordinal, delivery) in route.deliveries.iter().enumerate() {
            check_cancelled(monitor)?;
            if scanned == MAX_ROUTE_WORK {
                truncated = true;
                break 'routes;
            }
            scanned += 1;
            delivered += 1;
            let Some(target) = delivery.receiver.and_then(|receiver| receiver_position(graph, receiver)) else {
                continue;
            };
            relevant += 1;
            if steps.len() < ROUTE_STEPS {
                let kind = match delivery.kind {
                    DeliveryKind::Update { .. } => "update",
                    DeliveryKind::Shape { .. } => "shape",
                };
                let view = match delivery.view {
                    OperandView::Captured => "captured",
                    OperandView::Live => "live",
                };
                let guard = if graph.guards.get(delivery.guard.index()) == Some(&Guard::Always) {
                    "unguarded"
                } else {
                    "guarded"
                };
                steps.push(format!(
                    "#{ordinal}/s{} {kind} -> ({}, {}, {}) [{view}, {guard}]",
                    delivery.sequence, target.x, target.y, target.z
                ));
            }
        }
        if !steps.is_empty() && details.len() < super::MAX_WITNESSES {
            let rest = relevant.saturating_sub(steps.len());
            details.push(format!(
                "Timing: route {route_id} ({}, {}, {}): {}{}",
                source.x,
                source.y,
                source.z,
                steps.join("; "),
                if rest == 0 { String::new() } else { format!("; +{rest}") }
            ));
        }
    }
    let cap = if truncated { "; scan stopped at work limit" } else { "" };
    Ok((
        vec![format!("Timing: {} routes; {delivered} deliveries scanned{cap}", graph.routes.len())],
        details,
    ))
}

fn receiver_position(graph: &CompileGraph, receiver: crate::redpiler::program_graph::CallbackReceiver) -> Option<BlockPos> {
    use crate::redpiler::program_graph::{CallbackReceiver, CircuitNodeKind};
    match receiver {
        CallbackReceiver::PistonBase(actor) | CallbackReceiver::PistonHead(actor) => piston_position(graph, actor),
        CallbackReceiver::Component(id) => {
            let node = graph.nodes.get(id.index())?;
            let relevant = matches!(
                &node.kind,
                CircuitNodeKind::Wire(_)
                    | CircuitNodeKind::Observer { .. }
                    | CircuitNodeKind::BuiltinReset { .. }
                    | CircuitNodeKind::Electrical(
                        crate::redpiler::components::NodeType::Wire | crate::redpiler::components::NodeType::Observer { .. }
                    )
            );
            relevant.then_some(node.position).flatten()
        }
        CallbackReceiver::Contact(_) => None,
    }
}

fn piston_position(graph: &CompileGraph, actor: crate::redpiler::program_graph::PistonId) -> Option<BlockPos> {
    graph.pistons.get(actor.index()).map(|piston| piston.position)
}

fn witness_position(graph: &CompileGraph, witness: LayerWitness) -> Option<BlockPos> {
    let node = match witness {
        LayerWitness::Actor(actor) | LayerWitness::PowerAtCut { actor, .. } => return piston_position(graph, actor),
        LayerWitness::Wire(node) => node,
        LayerWitness::Callback(occurrence) => graph.routes.get(occurrence.route.index())?.source,
        LayerWitness::Power(link) => match link.to {
            crate::redpiler::program_graph::PowerTarget::Input { node, .. } => node,
            crate::redpiler::program_graph::PowerTarget::Conductor { site, .. } => return graph.sites.get(site.index()).map(|site| site.position),
        },
    };
    graph.nodes.get(node.index()).and_then(|node| node.position)
}

fn decline_reason(reason: LayerDeclineReason) -> &'static str {
    match reason {
        LayerDeclineReason::ProofBudgetExceeded => "layer proof budget exceeded",
        LayerDeclineReason::SignalArrivalUnproven => "signal arrival is unproven",
        LayerDeclineReason::CallbackSemanticsUnsupported => "callback semantics unsupported",
        LayerDeclineReason::SharedOwnerOrderUnknown => "shared-owner FIFO order unknown",
        LayerDeclineReason::BoundaryEffectUnsupported => "boundary effect unsupported",
    }
}

fn check_cancelled(monitor: &TaskMonitor) -> Result<(), String> {
    if monitor.cancelled() {
        Err("cancelled while reporting nanotick diagnostics".into())
    } else {
        Ok(())
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

    fn graph() -> CompileGraph {
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

    fn node(position: BlockPos, kind: CircuitNodeKind) -> CircuitNodeDraft {
        CircuitNodeDraft {
            position: Some(position),
            raw_state: None,
            kind,
            state: ComponentState::Specialized,
            output_route: None,
        }
    }

    #[test]
    fn route_order_is_local_and_retains_event_view_and_guard() {
        let mut graph = graph();
        graph.nodes.push(node(
            BlockPos::new(1, 2, 3),
            CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever),
        ));
        graph.nodes.push(node(BlockPos::new(4, 5, 6), CircuitNodeKind::Wire(0)));
        graph.nodes.push(node(
            BlockPos::new(7, 8, 9),
            CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Observer {
                watched: BlockPos::new(7, 7, 9),
            }),
        ));
        graph.sites.extend([
            SiteSpec {
                position: BlockPos::new(4, 5, 6),
                source: SiteSource::Constant(ProfileId::new(0)),
                source_strength: 0,
            },
            SiteSpec {
                position: BlockPos::new(7, 8, 9),
                source: SiteSource::Constant(ProfileId::new(0)),
                source_strength: 0,
            },
        ]);
        graph.guards.push(Guard::Always);
        graph.guards.push(Guard::State(StateTest::PistonExtended(PistonId::new(0))));
        graph.routes.push(Route {
            source: NodeId::new(0),
            deliveries: vec![
                RouteDelivery {
                    target: SiteId::new(0),
                    kind: DeliveryKind::Update { direction: None },
                    guard: GuardId::new(0),
                    view: OperandView::Captured,
                    sequence: 0,
                    receiver: Some(CallbackReceiver::Component(NodeId::new(1))),
                },
                RouteDelivery {
                    target: SiteId::new(1),
                    kind: DeliveryKind::Shape {
                        direction: mchprs_blocks::BlockFace::Top,
                    },
                    guard: GuardId::new(1),
                    view: OperandView::Live,
                    sequence: 1,
                    receiver: Some(CallbackReceiver::Component(NodeId::new(2))),
                },
            ]
            .into(),
        });
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        let route = lines.iter().find(|line| line.starts_with("Timing: route 0 ")).unwrap();
        assert!(
            route.find("#0/s0 update -> (4, 5, 6) [captured, unguarded]").unwrap() < route.find("#1/s1 shape -> (7, 8, 9) [live, guarded]").unwrap()
        );
        assert!(route.contains("(1, 2, 3)") && route.contains("(4, 5, 6)") && route.contains("(7, 8, 9)"));
        assert!(lines.iter().any(|line| line.contains("route order is local, not absolute nanoticks")));
    }

    #[test]
    fn fifo_stages_and_declines_have_coordinates_without_absolute_time() {
        let mut graph = graph();
        graph.nodes.extend([
            node(BlockPos::new(2, 3, 4), CircuitNodeKind::Piston(PistonId::new(0))),
            node(BlockPos::new(5, 6, 7), CircuitNodeKind::Piston(PistonId::new(1))),
            node(BlockPos::new(8, 9, 10), CircuitNodeKind::Wire(0)),
        ]);
        let routes = PistonRoutes {
            base_changed: RouteId::new(0),
            near_changed: RouteId::new(0),
            far_changed: None,
            far_shape_changed: None,
            base_restored: RouteId::new(0),
            head_restored: RouteId::new(0),
            resource_restored_near: None,
            resource_restored_far: None,
        };
        graph.pistons = [BlockPos::new(2, 3, 4), BlockPos::new(5, 6, 7)]
            .into_iter()
            .enumerate()
            .map(|(index, position)| PistonSpec {
                node: NodeId::new(index as u32),
                position,
                facing: mchprs_blocks::BlockFace::North,
                sticky: false,
                base: ContactId::new(0),
                near: ContactId::new(0),
                far: ContactId::new(0),
                lane: PistonLane::Empty,
                routes,
            })
            .collect();
        graph.nodes.extend([
            node(
                BlockPos::new(11, 12, 13),
                CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Lever),
            ),
            node(BlockPos::new(14, 15, 16), CircuitNodeKind::Wire(1)),
        ]);
        graph.guards.push(Guard::Always);
        graph.sites.push(SiteSpec {
            position: BlockPos::new(14, 15, 16),
            source: SiteSource::Constant(ProfileId::new(0)),
            source_strength: 0,
        });
        graph.routes.push(Route {
            source: NodeId::new(3),
            deliveries: vec![RouteDelivery {
                target: SiteId::new(0),
                kind: DeliveryKind::Update { direction: None },
                guard: GuardId::new(0),
                view: OperandView::Live,
                sequence: 0,
                receiver: Some(CallbackReceiver::Component(NodeId::new(4))),
            }]
            .into(),
        });
        let usage = |actor| LayerUse {
            use_id: LayerUseId(actor),
            actor: PistonId::new(actor),
            requested: LayerExprId(0),
            accepted: LayerExprId(0),
            consumed_before: LayerExprId(0),
            consumed_after: LayerExprId(0),
            cause: UseCause {
                boundary_occurrences: vec![].into(),
            },
        };
        graph.plans.instant.layers = Some(LayeredInstantPlan {
            regions: vec![LayeredRegion {
                id: LayerRegionId(0),
                cells: vec![0, 1].into(),
                corridors: vec![].into(),
                stationary_entry: LayerExprId(0),
                inputs: vec![
                    LayerInput {
                        source: LayerInputSource::State(StateTest::PistonExtended(PistonId::new(0))),
                        role: LayerInputRole::RequestSample,
                    },
                    LayerInput {
                        source: LayerInputSource::State(StateTest::PistonExtended(PistonId::new(0))),
                        role: LayerInputRole::AcceptanceRevalidation,
                    },
                ]
                .into(),
                outputs: vec![].into(),
                graph_outputs: vec![].into(),
                resource_outputs: vec![].into(),
                cold_lifecycle_occurrences: vec![].into(),
                expressions: vec![].into(),
                pin_values: vec![].into(),
                layers: vec![CausalLayer {
                    order: LayerOrder::Fifo,
                    uses: vec![usage(0), usage(1)].into(),
                }]
                .into(),
            }]
            .into(),
            programs: vec![].into(),
            declines: (0..9)
                .map(|_| LayerDecline {
                    cells: vec![0].into(),
                    corridors: vec![].into(),
                    witness: LayerWitness::Wire(NodeId::new(2)),
                    reason: LayerDeclineReason::SharedOwnerOrderUnknown,
                })
                .collect(),
        });
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        let stage = lines.iter().find(|line| line.starts_with("Timing: region 0 FIFO stage")).unwrap();
        assert!(stage.find("#0 (2, 3, 4)").unwrap() < stage.find("#1 (5, 6, 7)").unwrap());
        assert!(stage.contains("(2, 3, 4)") && stage.contains("(5, 6, 7)"));
        assert!(lines
            .iter()
            .any(|line| line.contains("Timing: unknown at (8, 9, 10): shared-owner FIFO order unknown")));
        let details = lines
            .iter()
            .filter(|line| line.starts_with("Timing: route ") || line.starts_with("Timing: region ") || line.starts_with("Timing: unknown at "))
            .count();
        assert_eq!(details, super::super::MAX_WITNESSES);
        assert!(!lines.iter().any(|line| line.starts_with("Timing: route 0 ")));
        assert!(!lines.join("\n").contains("nanotick 0"));
    }

    #[test]
    fn cancellation_stops_route_scan() {
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(report(&graph(), &monitor).unwrap_err().contains("cancelled"));
    }

    #[test]
    fn prepared_o0_graph_reports_route_local_order() {
        let mut world = PlotWorld::from_chunks(
            0,
            0,
            (0..PLOT_WIDTH).flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z))).collect(),
            Default::default(),
        );
        let repeater = BlockPos::new(40, 30, 40);
        let wire = BlockPos::new(41, 30, 40);
        let observer = BlockPos::new(42, 30, 40);
        world.set_block(
            repeater,
            Block::RedstoneRepeater {
                repeater: RedstoneRepeater::default(),
            },
        );
        world.set_block(wire, Block::RedstoneWire { wire: Default::default() });
        world.set_block(
            observer,
            Block::Observer {
                observer: Default::default(),
            },
        );
        let options = CompilerOptions::default();
        assert_eq!(options.optimization_level, Some(0));
        let candidate = prepare_candidate_graph(&world, (repeater, observer), &[], &options, std::sync::Arc::new(TaskMonitor::default())).unwrap();
        let lines = report(&candidate.graph, &TaskMonitor::default()).unwrap();
        assert!(lines.iter().any(|line| line.starts_with("Timing: route ") && line.contains("#0/s")));
        assert!(lines.iter().any(|line| line.contains("(40, 30, 40)") && line.contains("(41, 30, 40)")));
    }
}
