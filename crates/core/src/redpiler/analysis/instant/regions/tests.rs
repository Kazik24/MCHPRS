use super::*;

fn row(owner: BoundaryOwner, direction: BoundaryDirection, kind: BoundaryPortKind) -> BoundaryPort {
    BoundaryPort { owner, direction, kind }
}

#[test]
fn paired_dependencies_join_cell_corridor_cell_but_common_fanout_does_not() {
    let monitor = TaskMonitor::default();
    let mut budget = WorkBudget::new(&monitor);
    let mut sets = DisjointSets::new(5, &mut budget).unwrap();
    let shared_observer = BoundaryPortKind::Callback {
        occurrence: RouteOccurrence {
            route: RouteId::new(4),
            ordinal: 0,
        },
    };
    let ports = [
        row(BoundaryOwner::Cell(0), BoundaryDirection::Output, BoundaryPortKind::Power { link: 10 }),
        row(BoundaryOwner::Corridor(0), BoundaryDirection::Input, BoundaryPortKind::Power { link: 10 }),
        row(
            BoundaryOwner::Corridor(0),
            BoundaryDirection::Output,
            BoundaryPortKind::Power { link: 11 },
        ),
        row(BoundaryOwner::Cell(1), BoundaryDirection::Input, BoundaryPortKind::Power { link: 11 }),
        row(BoundaryOwner::Cell(2), BoundaryDirection::Input, shared_observer),
        row(BoundaryOwner::Cell(3), BoundaryDirection::Input, shared_observer),
    ];

    connect_ports(&mut sets, 4, &ports, &mut budget).unwrap();

    let path = sets.root(0);
    assert_eq!(path, sets.root(1), "the corridor input joins its producing cell");
    assert_eq!(path, sets.root(4), "the corridor output joins its reading cell");
    assert_ne!(sets.root(2), sets.root(3), "input-only common fanout cannot fuse independent owners");
    assert_ne!(path, sets.root(2), "unrelated observer fanout remains outside the power path");
}

#[test]
fn only_matching_output_to_input_pairs_connect_owners() {
    let monitor = TaskMonitor::default();
    let mut budget = WorkBudget::new(&monitor);
    let mut sets = DisjointSets::new(6, &mut budget).unwrap();
    let ports = [
        row(BoundaryOwner::Cell(0), BoundaryDirection::Output, BoundaryPortKind::Power { link: 3 }),
        row(BoundaryOwner::Cell(1), BoundaryDirection::Input, BoundaryPortKind::Power { link: 3 }),
        row(BoundaryOwner::Cell(2), BoundaryDirection::Output, BoundaryPortKind::Power { link: 4 }),
        row(BoundaryOwner::Cell(3), BoundaryDirection::Output, BoundaryPortKind::Power { link: 4 }),
        row(BoundaryOwner::Cell(4), BoundaryDirection::Output, BoundaryPortKind::Power { link: 5 }),
        row(BoundaryOwner::Cell(5), BoundaryDirection::Input, BoundaryPortKind::Power { link: 6 }),
    ];

    connect_ports(&mut sets, 6, &ports, &mut budget).unwrap();

    assert_eq!(sets.root(0), sets.root(1), "the exact source/reader pair is connected");
    assert_ne!(sets.root(2), sets.root(3), "multiple producers without a reader stay separate");
    assert_ne!(sets.root(4), sets.root(5), "opposite directions with different source IDs do not pair");
}

#[test]
fn unsupported_owners_and_observations_are_cuts_without_erasing_port_rows() {
    let monitor = TaskMonitor::default();
    let mut budget = WorkBudget::new(&monitor);
    let mut sets = DisjointSets::new(4, &mut budget).unwrap();
    let observation = BoundaryPortKind::GuardState {
        guard: GuardId::new(0),
        test: StateTest::ResourceEnabled(ResourceId::new(0)),
    };
    let ports = [
        row(BoundaryOwner::Cell(0), BoundaryDirection::Output, BoundaryPortKind::Power { link: 1 }),
        row(BoundaryOwner::Cell(1), BoundaryDirection::Input, BoundaryPortKind::Power { link: 1 }),
        row(BoundaryOwner::Cell(1), BoundaryDirection::Output, BoundaryPortKind::Power { link: 2 }),
        row(BoundaryOwner::Cell(2), BoundaryDirection::Input, BoundaryPortKind::Power { link: 2 }),
        row(BoundaryOwner::Cell(1), BoundaryDirection::Output, observation),
        row(BoundaryOwner::Cell(3), BoundaryDirection::Input, observation),
    ];
    let original_ports = ports;
    connect_selected_ports(&mut sets, 4, &ports, &mut budget, &[true, true, false, true], |kind| {
        matches!(kind, BoundaryPortKind::Power { .. })
    })
    .unwrap();
    assert_eq!(sets.root(0), sets.root(1), "the certified computation still composes");
    assert_ne!(sets.root(1), sets.root(2), "the unsupported owner stays outside");
    assert_ne!(sets.root(1), sets.root(3), "an observation remains a boundary dependency");
    assert_eq!(ports, original_ports, "cutting composition never erases its source obligations");
}
