//! Read-only inventory of piston lifecycle evidence that is not yet a complete certificate.
use crate::redpiler::program_graph::{CellDeclineReason, CompileGraph};
use crate::redpiler::TaskMonitor;

pub(super) fn report(graph: &CompileGraph, monitor: &TaskMonitor) -> Result<Vec<String>, String> {
    if monitor.cancelled() {
        return Err("unproven diagnostics cancelled".into());
    }
    let catalog = graph.plans.instant.boundaries.as_ref();
    let mut actor_cells = vec![None; graph.pistons.len()];
    let mut actor_declines = vec![None; graph.pistons.len()];
    let mut with_cell = 0usize;
    let mut declined = 0usize;

    if let Some(catalog) = catalog {
        for (cell_index, cell) in catalog.cells.iter().enumerate() {
            if cell_index & 0x3ff == 0 && monitor.cancelled() {
                return Err("unproven diagnostics cancelled".into());
            }
            for (use_index, use_) in cell.uses.iter().enumerate() {
                if use_index & 0x3ff == 0 && monitor.cancelled() {
                    return Err("unproven diagnostics cancelled".into());
                }
                if let Some(slot) = actor_cells.get_mut(use_.actor.index()) {
                    *slot = Some(cell_index);
                }
            }
        }
        for (decline_index, decline) in catalog.declines.iter().enumerate() {
            if decline_index & 0x3ff == 0 && monitor.cancelled() {
                return Err("unproven diagnostics cancelled".into());
            }
            for (actor_index, &actor) in decline.actors.iter().enumerate() {
                if actor_index & 0x3ff == 0 && monitor.cancelled() {
                    return Err("unproven diagnostics cancelled".into());
                }
                if let Some(slot) = actor_declines.get_mut(actor.index()) {
                    *slot = Some(decline.reason);
                }
            }
        }
    }

    let mut details = Vec::new();
    for (index, piston) in graph.pistons.iter().enumerate() {
        if index & 0x3ff == 0 && monitor.cancelled() {
            return Err("unproven diagnostics cancelled".into());
        }
        if actor_cells[index].is_some() {
            with_cell += 1;
        }
        if actor_declines[index].is_some() {
            declined += 1;
        }
        if details.len() < super::MAX_WITNESSES {
            let evidence = if let Some(cell) = actor_cells[index] {
                format!("local pull-cell evidence (cell {cell})")
            } else if let Some(reason) = actor_declines[index] {
                format!("admission declined: {}", decline_reason(reason))
            } else if catalog.is_some() {
                "no pull-cell evidence".to_owned()
            } else {
                "catalog unavailable".to_owned()
            };
            details.push(format!(
                "piston at ({}, {}, {}): {evidence}",
                piston.position.x, piston.position.y, piston.position.z
            ));
        }
    }
    if monitor.cancelled() {
        return Err("unproven diagnostics cancelled".into());
    }

    let mut lines = vec![format!(
        "Unproven: {} pistons; local evidence {}; declined {}",
        graph.pistons.len(),
        with_cell,
        declined
    )];
    let shown = details.len();
    lines.extend(details);
    if shown < graph.pistons.len() {
        lines.push(format!("Showing {shown}/{}", graph.pistons.len()));
    }
    Ok(lines)
}

fn decline_reason(reason: CellDeclineReason) -> String {
    match reason {
        CellDeclineReason::UnrecognizedActor(actor) => format!("actor {}: reset not recognized", actor.index()),
        CellDeclineReason::DifferentPullSources(resource) => format!("resource {}: different Far sources", resource.index()),
        CellDeclineReason::NoPullSource(resource) => format!("resource {}: no supported pull lane", resource.index()),
        CellDeclineReason::MissingDeferredWake(actor) => format!("actor {}: no restore route", actor.index()),
        CellDeclineReason::PinTableBudgetExceeded => "pin table budget exceeded".into(),
        CellDeclineReason::ConflictingSourceContact(contact) => format!("contact {}: conflicting payloads", contact.index()),
        CellDeclineReason::UnsupportedMaterial(resource) => format!("resource {}: unsupported material", resource.index()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redpiler::program_graph::*;
    use mchprs_blocks::{BlockFace, BlockPos};

    fn piston(index: u32) -> PistonSpec {
        PistonSpec {
            node: NodeId::new(index),
            position: BlockPos::new(index as i32, 2, 3),
            facing: BlockFace::North,
            sticky: false,
            base: ContactId::new(0),
            near: ContactId::new(1),
            far: ContactId::new(2),
            lane: PistonLane::Empty,
            routes: PistonRoutes {
                base_changed: RouteId::new(0),
                near_changed: RouteId::new(1),
                far_changed: None,
                far_shape_changed: None,
                base_restored: RouteId::new(2),
                head_restored: RouteId::new(3),
                resource_restored_near: None,
                resource_restored_far: None,
            },
        }
    }

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
    fn empty_graph_has_no_actor_details() {
        let lines = report(&empty_graph(), &TaskMonitor::default()).unwrap();
        assert_eq!(lines[0], "Unproven: 0 pistons; local evidence 0; declined 0");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn missing_catalog_is_not_reported_as_non_instant() {
        let mut graph = empty_graph();
        graph.pistons.push(piston(0));
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        assert!(lines[1].contains("catalog unavailable"));
        assert_eq!(lines[0], "Unproven: 1 pistons; local evidence 0; declined 0");
    }

    #[test]
    fn explicit_decline_is_reported_without_claiming_lifecycle_proof() {
        let mut graph = empty_graph();
        let actor = PistonId::new(0);
        graph.pistons.push(piston(0));
        graph.plans.instant.boundaries = Some(BoundaryCatalog {
            declines: vec![CellDecline {
                actors: vec![actor],
                reason: CellDeclineReason::UnrecognizedActor(actor),
            }]
            .into_boxed_slice(),
            ..BoundaryCatalog::default()
        });
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        assert!(lines[1].contains("actor 0: reset not recognized"));
        assert_eq!(lines[0], "Unproven: 1 pistons; local evidence 0; declined 1");
    }

    #[test]
    fn local_cell_contract_is_not_complete_lifecycle_proof() {
        let mut graph = empty_graph();
        graph.pistons.push(piston(0));
        let actor = PistonId::new(0);
        graph.plans.instant.boundaries = Some(BoundaryCatalog {
            cells: vec![CellContract {
                uses: vec![CellUse {
                    actor,
                    reset: CellReset::Observer {
                        node: NodeId::new(1),
                        wake: CellResetWake {
                            watched_actor: actor,
                            on_base_changed: Box::new([]),
                            on_base_restored: Box::new([]),
                        },
                    },
                    powered: CellPinId(0),
                    requested: CellPinId(0),
                    valid: CellPinId(0),
                    activations: Box::new([]),
                    retraction: CellRetractionEffect {
                        actor_revision_increment: 1,
                        transfer: None,
                        routes: CellRetractionRoutes {
                            base_changed: RouteId::new(0),
                            far_changed: None,
                            near_changed: RouteId::new(1),
                        },
                        wake: CellDeferredWake {
                            ticks_after_acceptance: 2,
                            base_restored: RouteId::new(2),
                            resource: None,
                        },
                    },
                }]
                .into_boxed_slice(),
                pins: Box::new([]),
                stationary_entry: CellPinId(0),
                resources: Box::new([]),
            }]
            .into_boxed_slice(),
            ..BoundaryCatalog::default()
        });
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        assert!(lines[1].contains("local pull-cell evidence"));
        assert!(lines[0].starts_with("Unproven: 1 pistons; local evidence 1; declined 0"));
    }

    #[test]
    fn actor_details_are_bounded() {
        let mut graph = empty_graph();
        graph
            .pistons
            .extend((0..super::super::MAX_WITNESSES + 3).map(|index| piston(index as u32)));
        let lines = report(&graph, &TaskMonitor::default()).unwrap();
        assert_eq!(lines.len(), super::super::MAX_WITNESSES + 2);
        assert_eq!(lines.last().unwrap(), "Showing 8/11");
    }

    #[test]
    fn cancelled_scan_stops() {
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert_eq!(report(&empty_graph(), &monitor).unwrap_err(), "unproven diagnostics cancelled");
    }
}
