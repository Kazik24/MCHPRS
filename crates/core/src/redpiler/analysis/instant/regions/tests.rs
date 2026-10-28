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

#[test]
fn cutting_one_cell_keeps_other_joinable_regions_and_all_port_rows() {
    let monitor = TaskMonitor::default();
    let graph = CompileGraph {
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
    };
    let empty_cell = || CellContract {
        uses: Box::new([]),
        pins: Box::new([]),
        stationary_entry: CellPinId(0),
        resources: Box::new([]),
    };
    let dependency = |link| BoundaryPortKind::Power { link };
    let catalog = BoundaryCatalog {
        cells: (0..5).map(|_| empty_cell()).collect::<Vec<_>>().into_boxed_slice(),
        ports: vec![
            row(BoundaryOwner::Cell(0), BoundaryDirection::Output, dependency(0)),
            row(BoundaryOwner::Cell(1), BoundaryDirection::Output, dependency(0)),
            row(BoundaryOwner::Cell(2), BoundaryDirection::Input, dependency(0)),
            row(BoundaryOwner::Cell(3), BoundaryDirection::Output, dependency(1)),
            row(BoundaryOwner::Cell(4), BoundaryDirection::Input, dependency(1)),
        ]
        .into_boxed_slice(),
        ..Default::default()
    };

    let partition = super::partition_with_cuts(&graph, &catalog, &monitor, &[false, true, true, true, true], 0, |_| true).unwrap();
    assert_ne!(
        partition.owner_region[0], partition.owner_region[1],
        "the rejected source owner stays separate"
    );
    assert_eq!(
        partition.owner_region[1], partition.owner_region[2],
        "the unaffected neighbor pair still composes"
    );
    assert_eq!(
        partition.owner_region[3], partition.owner_region[4],
        "an unrelated valid pair still composes"
    );
    assert_ne!(partition.owner_region[1], partition.owner_region[3]);
    assert_eq!(
        partition.regions.iter().map(|region| region.ports.len()).sum::<usize>(),
        catalog.ports.len()
    );
}

fn empty_event_graph() -> CompileGraph {
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

fn event_use(input: RouteOccurrence, output: RouteId, deferred: RouteId) -> CellUse {
    CellUse {
        actor: PistonId::new(0),
        reset: CellReset::Payload {
            resource: ResourceId::new(0),
            notification: input,
            sample: input,
        },
        powered: CellPinId(0),
        requested: CellPinId(0),
        valid: CellPinId(0),
        activations: vec![input].into_boxed_slice(),
        retraction: CellRetractionEffect {
            actor_revision_increment: 1,
            transfer: None,
            routes: CellRetractionRoutes {
                base_changed: output,
                near_changed: output,
                far_changed: None,
            },
            wake: CellDeferredWake {
                ticks_after_acceptance: 2,
                base_restored: deferred,
                resource: None,
            },
        },
    }
}

#[test]
fn greedy_events_keep_connected_and_independent_paths_and_cut_unsupported_owners() {
    let event = |route, ordinal| RouteOccurrence {
        route: RouteId::new(route),
        ordinal,
    };
    let input_a = event(0, 0);
    let bridge = event(1, 0);
    let output = event(2, 0);
    let input_b = event(4, 0);
    let independent_output = event(5, 0);
    let cell = |use_| CellContract {
        uses: vec![use_].into_boxed_slice(),
        pins: Box::new([]),
        stationary_entry: CellPinId(0),
        resources: Box::new([]),
    };
    let callback = |occurrence| BoundaryPortKind::Callback { occurrence };
    let catalog = BoundaryCatalog {
        cells: vec![
            cell(event_use(input_a, bridge.route, RouteId::new(3))),
            cell(event_use(bridge, output.route, RouteId::new(6))),
            cell(event_use(input_b, independent_output.route, RouteId::new(7))),
        ]
        .into_boxed_slice(),
        ports: vec![
            row(BoundaryOwner::Cell(0), BoundaryDirection::Input, callback(input_a)),
            row(BoundaryOwner::Cell(0), BoundaryDirection::Output, callback(bridge)),
            row(BoundaryOwner::Cell(1), BoundaryDirection::Input, callback(bridge)),
            row(BoundaryOwner::Cell(1), BoundaryDirection::Output, callback(output)),
            row(BoundaryOwner::Cell(2), BoundaryDirection::Input, callback(input_b)),
            row(BoundaryOwner::Cell(2), BoundaryDirection::Output, callback(independent_output)),
        ]
        .into_boxed_slice(),
        ..Default::default()
    };
    let graph = empty_event_graph();
    let monitor = TaskMonitor::default();
    let (selected, edges) = greedy_event_owners(&graph, &catalog, &[true, true, true], 0, &monitor).unwrap();
    assert_eq!(selected, [true, true, true]);
    assert_eq!(edges, [bridge]);
    let (selected, edges) = greedy_event_owners(&graph, &catalog, &[true, false, true], 0, &monitor).unwrap();
    assert_eq!(selected, [true, false, true]);
    assert!(edges.is_empty());
    let index = EventIndex::new(&catalog, &[true, true, true], &mut WorkBudget::new(&monitor)).unwrap();
    assert!(!index.accepts(&catalog, 1, event(1, 1)), "another delivery ordinal is not an activation");
    assert_eq!(index.phase(&graph, &catalog, 0, input_a, event(3, 0)), EventPhase::Deferred);
    assert_eq!(index.phase(&graph, &catalog, 0, event(0, 1), bridge), EventPhase::Unrelated);
    monitor.cancel();
    assert!(greedy_event_owners(&graph, &catalog, &[true, true, true], 0, &monitor).is_err());
}

#[test]
fn greedy_events_follow_immediate_same_owner_activations() {
    let event = |route, ordinal| RouteOccurrence {
        route: RouteId::new(route),
        ordinal,
    };
    let input = event(0, 0);
    let internal = event(1, 7);
    let output = event(2, 0);
    let catalog = BoundaryCatalog {
        cells: vec![CellContract {
            uses: vec![
                event_use(input, internal.route, RouteId::new(3)),
                event_use(internal, output.route, RouteId::new(4)),
            ]
            .into_boxed_slice(),
            pins: Box::new([]),
            stationary_entry: CellPinId(0),
            resources: Box::new([]),
        }]
        .into_boxed_slice(),
        ports: vec![
            row(
                BoundaryOwner::Cell(0),
                BoundaryDirection::Input,
                BoundaryPortKind::Callback { occurrence: input },
            ),
            row(
                BoundaryOwner::Cell(0),
                BoundaryDirection::Output,
                BoundaryPortKind::Callback { occurrence: output },
            ),
        ]
        .into_boxed_slice(),
        ..Default::default()
    };
    let (selected, edges) = greedy_event_owners(&empty_event_graph(), &catalog, &[true], 0, &TaskMonitor::default()).unwrap();
    assert_eq!(selected, [true]);
    assert!(edges.is_empty(), "same-owner callbacks must not create a cross-owner merge");
}

#[test]
fn greedy_events_trim_dead_reads_and_keep_shared_uses_in_one_owner() {
    let event = |route, ordinal| RouteOccurrence {
        route: RouteId::new(route),
        ordinal,
    };
    let first = event(0, 0);
    let second = event(1, 0);
    let dead = event(2, 0);
    let first_effect = event(3, 0);
    let second_effect = event(4, 0);
    let mut other_use = event_use(second, second_effect.route, RouteId::new(9));
    other_use.actor = PistonId::new(1);
    let catalog = BoundaryCatalog {
        cells: vec![
            CellContract {
                uses: vec![event_use(first, first_effect.route, RouteId::new(8)), other_use].into_boxed_slice(),
                pins: Box::new([]),
                stationary_entry: CellPinId(0),
                resources: Box::new([]),
            },
            CellContract {
                uses: vec![event_use(dead, RouteId::new(5), RouteId::new(10))].into_boxed_slice(),
                pins: Box::new([]),
                stationary_entry: CellPinId(0),
                resources: Box::new([]),
            },
        ]
        .into_boxed_slice(),
        ports: vec![
            row(
                BoundaryOwner::Cell(0),
                BoundaryDirection::Input,
                BoundaryPortKind::Callback { occurrence: first },
            ),
            row(
                BoundaryOwner::Cell(0),
                BoundaryDirection::Output,
                BoundaryPortKind::Callback { occurrence: first_effect },
            ),
            row(
                BoundaryOwner::Cell(0),
                BoundaryDirection::Output,
                BoundaryPortKind::Callback { occurrence: second_effect },
            ),
            row(
                BoundaryOwner::Cell(1),
                BoundaryDirection::Input,
                BoundaryPortKind::Callback { occurrence: dead },
            ),
            row(BoundaryOwner::Cell(1), BoundaryDirection::Input, BoundaryPortKind::Power { link: 0 }),
        ]
        .into_boxed_slice(),
        ..Default::default()
    };
    let graph = empty_event_graph();
    let monitor = TaskMonitor::default();
    let (selected, edges) = greedy_event_owners(&graph, &catalog, &[true, true], 0, &monitor).unwrap();
    assert_eq!(
        selected,
        [true, false],
        "a read with no event-qualified output does not make a dead owner useful"
    );
    assert!(edges.is_empty(), "a power read is not an execution edge");
    let index = EventIndex::new(&catalog, &[true, true], &mut WorkBudget::new(&monitor)).unwrap();
    assert_eq!(index.phase(&graph, &catalog, 0, first, first_effect), EventPhase::Immediate);
    assert_eq!(index.phase(&graph, &catalog, 0, first, second_effect), EventPhase::Unrelated);
    assert_eq!(index.phase(&graph, &catalog, 0, second, second_effect), EventPhase::Immediate);
}

#[test]
fn unsupported_cell_does_not_consume_the_event_index_budget() {
    let occurrences = (0..10_000)
        .map(|ordinal| RouteOccurrence {
            route: RouteId::new(0),
            ordinal,
        })
        .collect::<Vec<_>>();
    let mut use_ = event_use(occurrences[0], RouteId::new(1), RouteId::new(2));
    use_.activations = occurrences.into_boxed_slice();
    let catalog = BoundaryCatalog {
        cells: vec![CellContract {
            uses: vec![use_].into_boxed_slice(),
            pins: Box::new([]),
            stationary_entry: CellPinId(0),
            resources: Box::new([]),
        }]
        .into_boxed_slice(),
        ..Default::default()
    };
    let monitor = TaskMonitor::default();
    let mut budget = WorkBudget::new(&monitor);
    EventIndex::new(&catalog, &[false], &mut budget).unwrap();
    assert_eq!(budget.work, 0);
    assert_eq!(budget.bytes, 0);
}

#[test]
fn explicit_growth_budget_scales_without_changing_default() {
    let ordinary = TaskMonitor::default();
    let mut one = WorkBudget::new(&ordinary);
    one.bytes = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 4;
    assert!(one.retain_bytes(1).is_err());
    one.work = MAX_REGION_WORK;
    assert!(one.visit().is_err());
    let expanded = TaskMonitor::default();
    expanded.set_budget_multiplier(4);
    let mut four = WorkBudget::new(&expanded);
    four.bytes = crate::redpiler::analysis::MAX_GRAPH_PREPARATION_BYTES / 4;
    assert!(four.retain_bytes(1).is_ok());
    four.work = MAX_REGION_WORK;
    assert!(four.visit().is_ok());
    expanded.cancel();
    assert!(four.visit().is_err());
}

#[test]
fn greedy_owner_edges_do_not_join_unreached_producers_or_constant_reads() {
    use mchprs_blocks::{BlockFace, BlockPos};
    let event = |route| RouteOccurrence {
        route: RouteId::new(route),
        ordinal: 0,
    };
    let a = event(0);
    let bridge = event(1);
    let b = event(2);
    let tail = event(3);
    let other_tail = event(4);
    let cell = |use_| CellContract {
        uses: vec![use_].into_boxed_slice(),
        pins: Box::new([]),
        stationary_entry: CellPinId(0),
        resources: Box::new([]),
    };
    let callback = |occurrence| BoundaryPortKind::Callback { occurrence };
    let catalog = BoundaryCatalog {
        cells: vec![
            cell(event_use(a, bridge.route, RouteId::new(8))),
            cell(event_use(bridge, tail.route, RouteId::new(9))),
            cell(event_use(b, other_tail.route, RouteId::new(10))),
        ]
        .into_boxed_slice(),
        ports: vec![
            row(BoundaryOwner::Cell(0), BoundaryDirection::Input, callback(a)),
            row(BoundaryOwner::Cell(0), BoundaryDirection::Output, callback(bridge)),
            row(BoundaryOwner::Cell(1), BoundaryDirection::Input, callback(bridge)),
            row(BoundaryOwner::Cell(1), BoundaryDirection::Output, callback(tail)),
            row(BoundaryOwner::Cell(2), BoundaryDirection::Input, callback(b)),
            row(BoundaryOwner::Cell(2), BoundaryDirection::Output, callback(other_tail)),
            // This producer is reached, but bridge is not one of its effects.
            row(BoundaryOwner::Cell(2), BoundaryDirection::Output, callback(bridge)),
            // A matching read of a constant must not become an owned edge.
            row(BoundaryOwner::Cell(0), BoundaryDirection::Output, BoundaryPortKind::Power { link: 0 }),
            row(BoundaryOwner::Cell(1), BoundaryDirection::Input, BoundaryPortKind::Power { link: 0 }),
        ]
        .into_boxed_slice(),
        ..Default::default()
    };
    let mut graph = empty_event_graph();
    graph.sites.push(SiteSpec {
        position: BlockPos::new(0, 0, 0),
        source: SiteSource::Constant(ProfileId::new(0)),
        source_strength: 15,
    });
    graph.power_links.push(PowerLink {
        from: SiteId::new(0),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: NodeId::new(0),
            port: InputPort::Main,
        },
        face: BlockFace::Top,
        attenuation: 0,
        guard: GuardId::new(0),
    });
    graph.pistons.push(PistonSpec {
        node: NodeId::new(0),
        position: BlockPos::new(0, 0, 0),
        facing: BlockFace::East,
        sticky: true,
        base: ContactId::new(0),
        near: ContactId::new(1),
        far: ContactId::new(2),
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
    let (selected, edges) = greedy_event_pairs(&graph, &catalog, &[true, true, true], 0, &TaskMonitor::default()).unwrap();
    assert_eq!(selected, [true, true, true]);
    assert_eq!(
        edges,
        vec![EventEdge {
            source: 0,
            target: 1,
            kind: callback(bridge)
        }]
    );
    graph.sites[0].source = SiteSource::Component(NodeId::new(0));
    let (_, edges) = greedy_event_pairs(&graph, &catalog, &[true, true, true], 0, &TaskMonitor::default()).unwrap();
    assert_eq!(
        edges,
        vec![
            EventEdge {
                source: 0,
                target: 1,
                kind: BoundaryPortKind::Power { link: 0 }
            },
            EventEdge {
                source: 0,
                target: 1,
                kind: callback(bridge)
            }
        ]
    );
}
