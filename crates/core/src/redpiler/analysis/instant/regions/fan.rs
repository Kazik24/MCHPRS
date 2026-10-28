//! Index typed boundary witnesses without changing their owners or order.
use super::*;

#[derive(Default)]
pub(super) struct PortEndpoints {
    pub(super) inputs: Vec<usize>,
    pub(super) outputs: Vec<usize>,
}

pub(super) fn index(
    cell_count: usize,
    owner_count: usize,
    ports: &[BoundaryPort],
    budget: &mut WorkBudget<'_>,
    include: impl Fn(usize, BoundaryPortKind) -> bool,
) -> Result<FxHashMap<BoundaryPortKind, PortEndpoints>, String> {
    let mut fans = FxHashMap::<BoundaryPortKind, PortEndpoints>::default();
    for port in ports {
        budget.visit()?;
        let owner = owner_index(port.owner, cell_count)?;
        if owner >= owner_count {
            return Err("instant boundary port references a missing owner".into());
        }
        if !include(owner, port.kind) {
            continue;
        }
        if !fans.contains_key(&port.kind) {
            budget.retain_bytes(4 * std::mem::size_of::<(BoundaryPortKind, PortEndpoints)>())?;
        }
        budget.retain_bytes(2 * std::mem::size_of::<usize>())?;
        let fan = fans.entry(port.kind).or_default();
        match port.direction {
            BoundaryDirection::Input => fan.inputs.push(owner),
            BoundaryDirection::Output => fan.outputs.push(owner),
        }
    }
    Ok(fans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_index_preserves_duplicate_occurrences_and_directions() {
        let monitor = TaskMonitor::default();
        let mut budget = WorkBudget::new(&monitor);
        let first = BoundaryPortKind::Callback {
            occurrence: RouteOccurrence {
                route: RouteId::new(2),
                ordinal: 0,
            },
        };
        let second = BoundaryPortKind::Callback {
            occurrence: RouteOccurrence {
                route: RouteId::new(2),
                ordinal: 1,
            },
        };
        let row = |owner, direction, kind| BoundaryPort {
            owner: BoundaryOwner::Cell(owner),
            direction,
            kind,
        };
        let rows = [
            row(0, BoundaryDirection::Output, first),
            row(1, BoundaryDirection::Input, first),
            row(1, BoundaryDirection::Input, first),
            row(1, BoundaryDirection::Input, second),
        ];
        let fans = index(2, 2, &rows, &mut budget, |_, _| true).unwrap();
        assert_eq!(fans[&first].outputs, [0]);
        assert_eq!(fans[&first].inputs, [1, 1]);
        assert_eq!(fans[&second].inputs, [1]);
        assert!(fans[&second].outputs.is_empty());
    }

    #[test]
    fn fan_index_charges_unique_entries_and_duplicate_rows() {
        let monitor = TaskMonitor::default();
        let kind = BoundaryPortKind::Callback {
            occurrence: RouteOccurrence {
                route: RouteId::new(0),
                ordinal: 0,
            },
        };
        let port = BoundaryPort {
            owner: BoundaryOwner::Cell(0),
            direction: BoundaryDirection::Input,
            kind,
        };
        let mut budget = WorkBudget::new(&monitor);
        let rows = vec![port; 1_024];
        let fans = index(1, 1, &rows, &mut budget, |_, _| true).unwrap();
        assert_eq!(fans[&kind].inputs.len(), rows.len());
        assert!(budget.bytes >= 2 * rows.len() * std::mem::size_of::<usize>());
        let mut exhausted = WorkBudget::new(&monitor);
        exhausted.bytes = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 4 - 1;
        assert!(index(1, 1, &[port], &mut exhausted, |_, _| true).is_err());
    }

    #[test]
    fn native_input_remains_a_read_without_an_owned_producer() {
        let monitor = TaskMonitor::default();
        let kind = BoundaryPortKind::Power { link: 4 };
        let port = BoundaryPort {
            owner: BoundaryOwner::Cell(0),
            direction: BoundaryDirection::Input,
            kind,
        };
        let fans = index(1, 1, &[port], &mut WorkBudget::new(&monitor), |_, _| true).unwrap();
        assert_eq!(fans[&kind].inputs, [0]);
        assert!(fans[&kind].outputs.is_empty());
    }

    #[test]
    fn filtering_retains_budget_only_for_selected_rows() {
        let monitor = TaskMonitor::default();
        let mut budget = WorkBudget::new(&monitor);
        let ports = [
            BoundaryPort {
                owner: BoundaryOwner::Cell(0),
                direction: BoundaryDirection::Input,
                kind: BoundaryPortKind::Power { link: 0 },
            },
            BoundaryPort {
                owner: BoundaryOwner::Cell(1),
                direction: BoundaryDirection::Output,
                kind: BoundaryPortKind::Power { link: 1 },
            },
        ];
        let fans = index(2, 2, &ports, &mut budget, |owner, _| owner == 0).unwrap();
        assert_eq!(fans.len(), 1);
        assert_eq!(budget.work, 2);
        assert_eq!(
            budget.bytes,
            4 * std::mem::size_of::<(BoundaryPortKind, PortEndpoints)>() + 2 * std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn fan_index_rejects_invalid_owners_and_honors_cancellation() {
        let monitor = TaskMonitor::default();
        let row = BoundaryPort {
            owner: BoundaryOwner::Corridor(1),
            direction: BoundaryDirection::Input,
            kind: BoundaryPortKind::Power { link: 0 },
        };
        assert!(index(1, 2, &[row], &mut WorkBudget::new(&monitor), |_, _| true).is_err());
        monitor.cancel();
        assert!(index(1, 2, &[row], &mut WorkBudget::new(&monitor), |_, _| true).is_err());
    }
}
