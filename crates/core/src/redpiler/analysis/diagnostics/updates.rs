//! Read-only checks for graph structures that can make update ordering sensitive.
use crate::redpiler::program_graph::{CircuitNodeKind, CompileGraph, InputPort, LinkKind, PowerTarget};
use crate::redpiler::TaskMonitor;

pub(super) fn report(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    if monitor.cancelled() {
        return Err("updates diagnostics cancelled".into());
    }
    let mut findings = Vec::new();
    let mut first_routes = vec![[None; 2]; graph.pistons.len()];
    let mut node_update_routes = vec![None; graph.nodes.len()];
    for (index, link) in graph.links.iter().enumerate() {
        if index & 0x3ff == 0 && monitor.cancelled() {
            return Err("updates diagnostics cancelled".into());
        }
        if !matches!(link.kind, LinkKind::Update { .. }) {
            continue;
        }
        node_update_routes[link.to.index()].get_or_insert(link.callback.route);
        let Some(CircuitNodeKind::Piston(actor)) = graph.nodes.get(link.to.index()).map(|node| &node.kind) else {
            continue;
        };
        let target = actor.index();
        if graph.pistons.get(target).is_some_and(|piston| piston.node == link.to) {
            let routes = &mut first_routes[target];
            if routes[0].is_none() {
                routes[0] = Some(link.callback.route);
            } else if routes[0] != Some(link.callback.route) && routes[1].is_none() {
                routes[1] = Some(link.callback.route);
            }
        }
    }

    let mut total = 0usize;
    for (index, routes) in first_routes.iter().enumerate() {
        if index & 0x3ff == 0 && monitor.cancelled() {
            return Err("updates diagnostics cancelled".into());
        }
        if routes[1].is_none() {
            continue;
        }
        total += 1;
        if findings.len() < super::MAX_WITNESSES {
            let pos = graph.pistons[index].position;
            let routes = routes.iter().flatten().map(|route| {
                    let source = graph
                        .routes
                        .get(route.index())
                        .and_then(|route| graph.nodes.get(route.source.index()))
                        .and_then(|node| node.position);
                    format!("{} from {}", route.index(), display_pos(source))
                })
                .collect::<Vec<_>>()
                .join(" / ");
            findings.push(format!(
                "Multi-update {}: routes {}",
                display_pos(Some(pos)),
                routes
            ));
        }
    }

    let mut qc_activations = 0usize;
    for link in &graph.power_links {
        if monitor.cancelled() {
            return Err("updates diagnostics cancelled".into());
        }
        let PowerTarget::Input {
            node,
            port: InputPort::Quasi,
        } = link.to
        else {
            continue;
        };
        let Some(route) = node_update_routes.get(node.index()).copied().flatten() else {
            continue;
        };
        qc_activations += 1;
        if findings.len() < super::MAX_WITNESSES {
            let target = graph.nodes.get(node.index()).and_then(|node| node.position);
            let source = graph
                .routes
                .get(route.index())
                .and_then(|route| graph.nodes.get(route.source.index()))
                .and_then(|node| node.position);
            let qc_source = graph.sites.get(link.from.index()).map(|site| site.position);
            findings.push(format!(
                "QC/update {}: data {} / route {} from {}",
                display_pos(target),
                display_pos(qc_source),
                route.index(),
                display_pos(source)
            ));
        }
    }

    // A movable resource appearing in multiple piston lanes gives those pistons a shared
    // occupancy dependency; this is a structural warning, not evidence of a bad execution.
    let mut contact_owners = vec![Vec::new(); graph.contacts.len()];
    for (i, piston) in graph.pistons.iter().enumerate() {
        if i & 0x3ff == 0 && monitor.cancelled() {
            return Err("updates diagnostics cancelled".into());
        }
        for contact in [piston.near, piston.far] {
            if let Some(owners) = contact_owners.get_mut(contact.index()) {
                owners.push((i, piston.position));
            }
        }
    }
    let mut shared = 0usize;
    let mut owner_seen = vec![usize::MAX; graph.pistons.len()];
    let mut shared_work = 0usize;
    let mut shared_scan_complete = true;
    for (resource_index, resource) in graph.resources.iter().enumerate() {
        if resource_index & 0x3ff == 0 && monitor.cancelled() {
            return Err("updates diagnostics cancelled".into());
        }
        let mut first_owner = None;
        let mut second_position = None;
        let mut owner_count = 0usize;
        for (contact_index, contact) in resource.contacts.iter().enumerate() {
            if contact_index & 0x3ff == 0 && monitor.cancelled() {
                return Err("updates diagnostics cancelled".into());
            }
            for &(owner, position) in contact_owners.get(contact.index()).into_iter().flatten() {
                shared_work += 1;
                if shared_work > 1_000_000 {
                    shared_scan_complete = false;
                    break;
                }
                if shared_work & 0x3ff == 0 && monitor.cancelled() {
                    return Err("updates diagnostics cancelled".into());
                }
                if owner_seen[owner] == resource_index {
                    continue;
                }
                owner_seen[owner] = resource_index;
                if first_owner.is_none() {
                    first_owner = Some(owner);
                } else if second_position.is_none() {
                    second_position = Some(position);
                }
                owner_count += 1;
            }
            if !shared_scan_complete {
                break;
            }
        }
        if owner_count > 1 {
            shared += 1;
            if findings.len() < super::MAX_WITNESSES {
                let first_position = first_owner.and_then(|i| graph.pistons.get(i)).map(|p| p.position);
                findings.push(format!(
                    "Shared resource {resource_index}: {} / {}",
                    display_pos(first_position),
                    display_pos(second_position)
                ));
            }
        }
        if !shared_scan_complete {
            break;
        }
    }
    if monitor.cancelled() {
        return Err("updates diagnostics cancelled".into());
    }
    let candidates = total + shared + qc_activations;
    let shown = findings.len();
    findings.insert(0, format!("Updates: {candidates} candidates"));
    if shown < candidates {
        findings.push(format!("Showing {shown}/{candidates}"));
    }
    if !shared_scan_complete {
        findings.push("Shared-resource scan incomplete; counts are lower bounds.".into());
    }
    Ok(findings)
}

fn display_pos(pos: Option<mchprs_blocks::BlockPos>) -> String {
    pos.map(|p| format!("({}, {}, {})", p.x, p.y, p.z))
        .unwrap_or_else(|| "unknown position".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::program_graph::{
        CallbackEdgeRef, CircuitNodeDraft, ComponentState, ContactId, ExecutionPlans, GuardId, LinkSpec, NodeId, PistonId, PistonLane, PistonRoutes,
        PistonSpec, PowerLink, PowerPort, PowerTarget, ProfileId, ResourceSpec, Route, RouteId, SiteSource, SiteSpec,
    };
    use mchprs_blocks::BlockFace;
    use mchprs_blocks::BlockPos;

    fn empty_graph() -> CompileGraph {
        CompileGraph {
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
            plans: ExecutionPlans::default(),
        }
    }

    #[test]
    fn empty_graph_reports_no_candidates() {
        let lines = report(&empty_graph(), &TaskMonitor::default()).unwrap();
        assert_eq!(lines[0], "Updates: 0 candidates");
    }

    #[test]
    fn cancelled_scan_stops() {
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert_eq!(report(&empty_graph(), &monitor).unwrap_err(), "updates diagnostics cancelled");
    }

    #[test]
    fn multiple_distinct_update_routes_to_piston_are_reported() {
        let mut graph = empty_graph();
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(1, 2, 3)),
            raw_state: None,
            kind: CircuitNodeKind::Piston(PistonId::new(0)),
            state: ComponentState::Specialized,
            output_route: None,
        });
        graph.pistons.push(PistonSpec {
            node: NodeId::new(0),
            position: BlockPos::new(1, 2, 3),
            facing: BlockFace::East,
            sticky: false,
            base: ContactId::new(0),
            near: ContactId::new(0),
            far: ContactId::new(0),
            lane: PistonLane::Empty,
            routes: PistonRoutes {
                base_changed: RouteId::new(0),
                near_changed: RouteId::new(0),
                far_changed: None,
                far_shape_changed: None,
                base_restored: RouteId::new(0),
                head_restored: RouteId::new(0),
                resource_restored_near: None,
                resource_restored_far: None,
            },
        });
        graph.nodes.push(CircuitNodeDraft {
            position: Some(BlockPos::new(0, 2, 3)),
            raw_state: None,
            kind: CircuitNodeKind::Electrical(crate::redpiler::components::NodeType::Torch),
            state: ComponentState::Specialized,
            output_route: None,
        });
        for (route, sequence) in [(0, 0), (1, 0), (1, 1)] {
            graph.links.push(LinkSpec {
                from: NodeId::new(1),
                to: NodeId::new(0),
                kind: LinkKind::Update {
                    direction: None,
                    guard: GuardId::new(0),
                },
                callback: CallbackEdgeRef {
                    route: RouteId::new(route),
                    sequence,
                },
            });
        }
        graph.routes.extend((0..2).map(|_| Route {
            source: NodeId::new(1),
            deliveries: Box::new([]),
        }));
        graph.routes.push(Route {
            source: NodeId::new(0),
            deliveries: Box::new([]),
        });
        graph.links.push(LinkSpec {
            from: NodeId::new(0),
            to: NodeId::new(1),
            kind: LinkKind::Update {
                direction: None,
                guard: GuardId::new(0),
            },
            callback: CallbackEdgeRef {
                route: RouteId::new(2),
                sequence: 0,
            },
        });
        graph.sites.push(SiteSpec {
            position: BlockPos::new(2, 2, 3),
            source: SiteSource::Constant(ProfileId::new(0)),
            source_strength: 0,
        });
        graph.power_links.push(PowerLink {
            from: crate::redpiler::program_graph::SiteId::new(0),
            from_port: PowerPort::Output,
            to: PowerTarget::Input {
                node: NodeId::new(1),
                port: crate::redpiler::program_graph::InputPort::Quasi,
            },
            face: BlockFace::West,
            attenuation: 0,
            guard: GuardId::new(0),
        });
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        assert_eq!(lines[0], "Updates: 2 candidates");
        assert!(lines.iter().any(|line| line.starts_with("Multi-update (1, 2, 3): routes 0 from (0, 2, 3) / 1 from (0, 2, 3)")));
        assert!(lines
            .iter()
            .any(|line| line == "QC/update (0, 2, 3): data (2, 2, 3) / route 2 from (1, 2, 3)"));
    }

    #[test]
    fn resource_shared_by_two_piston_lanes_is_reported_once() {
        let mut graph = empty_graph();
        graph.contacts.resize_with(1, || crate::redpiler::program_graph::ContactSpec {
            node: NodeId::new(0),
            position: BlockPos::new(0, 0, 0),
            variants: Box::new([]),
        });
        for (index, x) in [1, 2].into_iter().enumerate() {
            graph.nodes.push(CircuitNodeDraft {
                position: Some(BlockPos::new(x, 2, 3)),
                raw_state: None,
                kind: CircuitNodeKind::Piston(PistonId::new(index as u32)),
                state: ComponentState::Specialized,
                output_route: None,
            });
            graph.pistons.push(PistonSpec {
                node: NodeId::new(index as u32),
                position: BlockPos::new(x, 2, 3),
                facing: BlockFace::East,
                sticky: false,
                base: ContactId::new(0),
                near: ContactId::new(0),
                far: ContactId::new(0),
                lane: PistonLane::Empty,
                routes: PistonRoutes {
                    base_changed: RouteId::new(0),
                    near_changed: RouteId::new(0),
                    far_changed: None,
                    far_shape_changed: None,
                    base_restored: RouteId::new(0),
                    head_restored: RouteId::new(0),
                    resource_restored_near: None,
                    resource_restored_far: None,
                },
            });
        }
        graph.resources.push(ResourceSpec {
            material: ProfileId::new(0),
            contacts: vec![ContactId::new(0)].into_boxed_slice(),
        });
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        assert!(lines
            .iter()
            .any(|line| line.contains("Shared resource") && line.contains("(1, 2, 3)") && line.contains("(2, 2, 3)")));
    }

    #[test]
    fn update_witness_details_are_capped() {
        let mut graph = empty_graph();
        for index in 0..10u32 {
            let node = NodeId::new(index);
            let pos = BlockPos::new(index as i32, 2, 3);
            graph.nodes.push(CircuitNodeDraft {
                position: Some(pos),
                raw_state: None,
                kind: CircuitNodeKind::Piston(PistonId::new(index)),
                state: ComponentState::Specialized,
                output_route: None,
            });
            graph.pistons.push(PistonSpec {
                node,
                position: pos,
                facing: BlockFace::East,
                sticky: false,
                base: ContactId::new(0),
                near: ContactId::new(0),
                far: ContactId::new(0),
                lane: PistonLane::Empty,
                routes: PistonRoutes {
                    base_changed: RouteId::new(0),
                    near_changed: RouteId::new(0),
                    far_changed: None,
                    far_shape_changed: None,
                    base_restored: RouteId::new(0),
                    head_restored: RouteId::new(0),
                    resource_restored_near: None,
                    resource_restored_far: None,
                },
            });
            for route in [index * 2, index * 2 + 1] {
                graph.links.push(LinkSpec {
                    from: node,
                    to: node,
                    kind: LinkKind::Update {
                        direction: None,
                        guard: GuardId::new(0),
                    },
                    callback: CallbackEdgeRef {
                        route: RouteId::new(route),
                        sequence: 0,
                    },
                });
                graph.routes.push(Route {
                    source: node,
                    deliveries: Box::new([]),
                });
            }
        }
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        assert_eq!(
            lines.iter().filter(|line| line.starts_with("Multi-update ")).count(),
            super::super::MAX_WITNESSES
        );
        assert_eq!(lines[0], "Updates: 10 candidates");
        assert!(lines.iter().any(|line| line == "Showing 8/10"));
    }
}
