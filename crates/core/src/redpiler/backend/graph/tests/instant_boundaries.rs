//! The catalog is checked against pinned fixtures and independently injected boundary dependencies.
use super::*;
use crate::redpiler::CompilerOptions;
use crate::redpiler::analysis::instant::boundaries::infer_boundaries;
use crate::redpiler::passes::planning::PlanBuilder;
use std::collections::{BTreeSet, HashSet};

fn prepare(id: &'static str) -> passes::rich::PreparedGraph {
    let circuit = boundary_contract_tests::load_mechanism(id);
    let monitor = TaskMonitor::default();
    let mut prepared = passes::rich::prepare(&circuit.build(), circuit.bounds(), vec![], &monitor, AnalysisLimits::default()).unwrap();
    let options = CompilerOptions {
        optimization_level: Some(3),
        stable_instant_inputs: true,
        ..Default::default()
    };
    passes::run_graph_pipeline(
        &mut prepared.graph,
        &options,
        &prepared.entry,
        &passes::experimental_boundary_pipeline(&options),
        &monitor,
    )
    .unwrap();
    prepared
}

fn assert_complete_ownership(graph: &CompileGraph, catalog: &BoundaryCatalog) {
    let mut actors: Vec<_> = catalog
        .cells
        .iter()
        .flat_map(|cell| cell.uses.iter().map(|use_| use_.actor.index()))
        .chain(
            catalog
                .declines
                .iter()
                .flat_map(|decline| decline.actors.iter().map(|actor| actor.index())),
        )
        .collect();
    actors.sort_unstable();
    assert_eq!(
        actors,
        (0..graph.pistons.len()).collect::<Vec<_>>(),
        "every actor is admitted or declined exactly once"
    );
    let mut wires = BTreeSet::new();
    for corridor in &catalog.corridors {
        for &wire in &corridor.wires {
            assert!(wires.insert(wire), "corridor ownership is exclusive");
            assert!(matches!(graph.nodes[wire.index()].kind, CircuitNodeKind::Wire(_)));
            assert!(
                !graph
                    .plans
                    .classic_nets
                    .iter()
                    .any(|net| net.wires.iter().any(|owned| owned.node == wire))
            );
        }
    }
}

fn owner_of_node(graph: &CompileGraph, catalog: &BoundaryCatalog, node: NodeId) -> Option<BoundaryOwner> {
    catalog
        .cells
        .iter()
        .enumerate()
        .find_map(|(index, cell)| {
            cell.uses
                .iter()
                .any(|use_| graph.pistons[use_.actor.index()].node == node || use_.reset.observer() == Some(node))
                .then_some(BoundaryOwner::Cell(index as u32))
        })
        .or_else(|| {
            catalog
                .corridors
                .iter()
                .enumerate()
                .find_map(|(index, corridor)| corridor.wires.contains(&node).then_some(BoundaryOwner::Corridor(index as u32)))
        })
}

fn has_port(catalog: &BoundaryCatalog, owner: BoundaryOwner, direction: BoundaryDirection, kind: BoundaryPortKind) -> bool {
    catalog
        .ports
        .iter()
        .any(|port| port.owner == owner && port.direction == direction && port.kind == kind)
}

#[test]
fn all_io_fixtures_export_deterministic_boundary_catalogs_and_account_for_every_actor() {
    let directory = root().join("target/layered-instant-boundaries");
    std::fs::create_dir_all(&directory).unwrap();
    for id in super::instant_contract_tests::IO_FIXTURES {
        let prepared = prepare(id);
        let graph = &prepared.graph;
        assert!(
            graph.plans.instant.regions.is_empty() && graph.plans.instant.proxies.is_empty(),
            "one authoritative experimental owner map"
        );
        let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
        assert_complete_ownership(graph, catalog);
        assert_eq!(
            *catalog,
            infer_boundaries(graph, &TaskMonitor::default()).unwrap(),
            "{id}: deterministic inference"
        );
        let actors: usize = catalog.cells.iter().map(|cell| cell.uses.len()).sum();
        if let Some(expected) = match id {
            "adder_1bit" => Some(13),
            "adder_11bits" => Some(142),
            _ => None,
        } {
            assert_eq!(actors, expected, "all adder actors retain contracts");
        }
        let inputs = catalog.ports.iter().filter(|port| port.direction == BoundaryDirection::Input).count();
        let outputs = catalog.ports.len() - inputs;
        println!(
            "{id}: cells={}, actors={actors}/{}, corridors={}, inputs={inputs}, outputs={outputs}, declines={}",
            catalog.cells.len(),
            graph.pistons.len(),
            catalog.corridors.len(),
            catalog.declines.len()
        );
        let export = crate::redpiler::analysis::export_json::from_graph(&prepared.report, graph).unwrap();
        std::fs::write(directory.join(format!("{id}.graph.json")), export.to_bytes().unwrap()).unwrap();
    }
}

#[test]
fn adder_controls_and_timed_repeaters_have_distinct_boundary_bindings() {
    for id in ["adder_1bit", "adder_11bits"] {
        let circuit = boundary_contract_tests::load_mechanism(id);
        let prepared = prepare(id);
        let graph = &prepared.graph;
        let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
        let mut controls = HashSet::new();
        let mut repeaters = HashSet::new();
        for port in &catalog.ports {
            if port.direction == BoundaryDirection::Input {
                let source = match port.kind {
                    BoundaryPortKind::Callback { occurrence } => Some(graph.routes[occurrence.route.index()].source),
                    BoundaryPortKind::WireNeighborhood { neighbor, .. } | BoundaryPortKind::CapturedWire { neighbor, .. } => {
                        match graph.sites[neighbor.index()].source {
                            SiteSource::Component(node) => Some(node),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(node) = source {
                    if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lever)) {
                        controls.insert(graph.nodes[node.index()].position.unwrap());
                    }
                }
            }
            let BoundaryPortKind::Power { link } = port.kind else { continue };
            let link = graph.power_links[link as usize];
            if port.direction == BoundaryDirection::Input {
                if let SiteSource::Component(node) = graph.sites[link.from.index()].source {
                    if matches!(graph.nodes[node.index()].kind, CircuitNodeKind::Electrical(NodeType::Lever)) {
                        controls.insert(graph.nodes[node.index()].position.unwrap());
                    }
                }
            } else if let PowerTarget::Input { node, .. } = link.to {
                if matches!(
                    graph.nodes[node.index()].raw_state.map(Block::from_id),
                    Some(Block::RedstoneRepeater { .. })
                ) {
                    repeaters.insert(graph.nodes[node.index()].position.unwrap());
                }
            }
        }
        for name in ["A", "B", "Cin", "trigger"] {
            for position in circuit.port_positions("inputs", name) {
                assert!(controls.contains(&position), "{id}: missing input control {name} at {position:?}");
            }
        }
        for name in ["sum_repeater", "carry_repeater"] {
            for position in circuit.port_positions("observations", name) {
                assert!(repeaters.contains(&position), "{id}: missing timed output {name} at {position:?}");
            }
        }
        assert_eq!(
            controls.len(),
            if id == "adder_1bit" { 4 } else { 23 },
            "data controls and trigger stay distinct"
        );
    }
}

#[test]
fn common_external_power_preserves_separate_cell_ownership() {
    let (mut graph, control) = boundary_contract_tests::independent_devices();
    let initial = infer_boundaries(&graph, &TaskMonitor::default()).unwrap();
    let before: Vec<_> = initial
        .cells
        .iter()
        .map(|cell| cell.uses.iter().map(|use_| use_.actor).collect::<Vec<_>>())
        .collect();
    let source = SiteId::new(graph.sites.iter().position(|site| site.source == SiteSource::Component(control)).unwrap() as u32);
    let always = GuardId::new(graph.guards.iter().position(|guard| *guard == Guard::Always).unwrap() as u32);
    for cell in &initial.cells {
        graph.power_links.push(PowerLink {
            from: source,
            from_port: PowerPort::Output,
            to: PowerTarget::Input {
                node: graph.pistons[cell.uses[0].actor.index()].node,
                port: InputPort::Main,
            },
            face: BlockFace::Top,
            attenuation: 0,
            guard: always,
        });
    }
    let after = infer_boundaries(&graph, &TaskMonitor::default()).unwrap();
    assert_eq!(
        after
            .cells
            .iter()
            .map(|cell| cell.uses.iter().map(|use_| use_.actor).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn nested_external_guard_is_an_input_to_both_sides_of_an_internal_transfer() {
    let mut prepared = prepare("adder_1bit");
    let graph = &mut prepared.graph;
    let original = graph.plans.instant.boundaries.as_ref().unwrap();
    let (link_index, source_owner, target_owner) = graph
        .power_links
        .iter()
        .enumerate()
        .find_map(|(index, link)| {
            let SiteSource::Component(source) = graph.sites[link.from.index()].source else {
                return None;
            };
            let PowerTarget::Input { node: target, .. } = link.to else { return None };
            let source = owner_of_node(graph, original, source)?;
            let target = owner_of_node(graph, original, target)?;
            (source != target).then_some((index, source, target))
        })
        .expect("fixture has wire-to-cell or cell-to-wire transfers");
    let outside=SiteId::new(graph.sites.iter().position(|site|matches!(site.source,SiteSource::Component(node) if matches!(graph.nodes[node.index()].kind,CircuitNodeKind::Electrical(NodeType::Lever)))).unwrap() as u32);
    let atom = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Site {
        site: outside,
        view: OperandView::Live,
        test: SiteTest::IsWire,
    });
    let nested = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Not(atom));
    graph.power_links[link_index].guard = nested;
    let catalog = infer_boundaries(graph, &TaskMonitor::default()).unwrap();
    let binding = BoundaryPortKind::GuardSite {
        guard: atom,
        site: outside,
        view: OperandView::Live,
        test: SiteTest::IsWire,
    };
    assert!(has_port(&catalog, source_owner, BoundaryDirection::Input, binding));
    assert!(has_port(&catalog, target_owner, BoundaryDirection::Input, binding));
}

#[test]
fn boundary_catalog_rejects_missing_witnesses_and_cancelled_publication() {
    let mut prepared = prepare("adder_1bit");
    let original = prepared.graph.plans.instant.boundaries.clone().unwrap();
    assert!(!original.ports.is_empty());
    prepared.graph.plans.instant.boundaries.as_mut().unwrap().ports = original.ports[1..].to_vec().into_boxed_slice();
    assert!(prepared.graph.validate().is_err(), "omitted dependency witness is rejected");
    prepared.graph.plans.instant.boundaries = Some(original.clone());
    let cancelled = TaskMonitor::default();
    cancelled.cancel();
    let options = CompilerOptions::default();
    let context = passes::PassContext::new(&cancelled, &options, &prepared.entry);
    assert!(
        passes::identify_instant_boundaries::IdentifyInstantBoundaries
            .run(&mut prepared.graph, &context)
            .is_err()
    );
    assert_eq!(
        prepared.graph.plans.instant.boundaries,
        Some(original),
        "cancelled builder publishes nothing"
    );
}

#[test]
fn bud_memory_is_outside_cells_while_its_power_and_update_channels_remain_distinct() {
    let circuit = boundary_contract_tests::load_mechanism("bud_instantmemorycellobserverupdate");
    let prepared = prepare(circuit.id);
    let graph = &prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let memory = circuit.place(local(&circuit.ports["observations"]["memory"]));
    let actor = PistonId::new(graph.pistons.iter().position(|actor| actor.position == memory).unwrap() as u32);
    assert!(catalog.declines.iter().any(|decline| decline.actors.contains(&actor)));
    assert!(catalog.cells.iter().all(|cell| cell.uses.iter().all(|use_| use_.actor != actor)));
    let node = graph.pistons[actor.index()].node;
    assert!(catalog.ports.iter().any(|port|matches!(port.kind,BoundaryPortKind::Power{link} if graph.power_links[link as usize].to==PowerTarget::Input{node,port:InputPort::Quasi})));
    // Sampling updates originate in the ordinary update piston/observer, not the data cell.
    let update_routes: Vec<_> = graph
        .routes
        .iter()
        .filter(|route| {
            route.deliveries.iter().any(|delivery| {
                matches!(delivery.kind, DeliveryKind::Update { .. })
                    && matches!(delivery.receiver, Some(CallbackReceiver::PistonBase(target) | CallbackReceiver::PistonHead(target)) if target==actor)
            })
        })
        .collect();
    assert!(!update_routes.is_empty());
    assert!(update_routes.iter().all(|route| owner_of_node(graph, catalog, route.source).is_none()));
}

#[test]
fn boundary_only_pipeline_preserves_both_adders_state_outputs_and_pending_work() {
    for id in ["adder_1bit", "adder_11bits"] {
        let circuit = boundary_contract_tests::load_mechanism(id);
        let prepared = prepare(id);
        let positions = circuit.positions();
        for case in super::instant_contract_tests::arithmetic_cases(id) {
            let mut native = circuit.build();
            let mut projected = circuit.build();
            let (program, state) = prepared.graph.clone().lower().unwrap();
            let mut backend = GraphBackend::new(program, state, 0).unwrap();
            let steps = case_steps(&circuit, &case, false, true)
                .into_iter()
                .chain(std::iter::repeat_n(Step::Tick, 32));
            for (index, step) in steps.enumerate() {
                let toggle = match step {
                    Step::SetLever { position, powered } => matches!(native.get_block(position),Block::Lever{lever} if lever.powered!=powered),
                    Step::Tick => false,
                };
                native_step(&mut native, step);
                graph_step(&mut backend, step, toggle).unwrap();
                check_boundary(
                    &backend,
                    &native,
                    &mut projected,
                    &positions,
                    &format!("{id} {} step {index}", case["id"]),
                )
                .unwrap();
            }
            // The observation scratch already contains projected future work. Reset into
            // a host world instead so its scheduler is not imported a second time.
            let mut handed_back = circuit.build();
            backend.reset(&mut handed_back);
            for tick in 0..8 {
                assert_eq!(
                    first_difference(&future(&native, &positions), &future(&handed_back, &positions)),
                    None,
                    "{id} {} handoff {tick}",
                    case["id"]
                );
                native.tick_interpreted();
                handed_back.tick_interpreted();
            }
        }
    }
}

#[test]
fn comparator_far_guard_and_head_observer_remain_observable_outputs() {
    use crate::redpiler::components::NodeState;
    use mchprs_blocks::BlockDirection;
    use mchprs_blocks::blocks::{ComparatorMode, RedstoneComparator};
    let mut prepared = prepare("adder_1bit");
    let graph = &mut prepared.graph;
    let contract = &graph.plans.instant.boundaries.as_ref().unwrap().cells[0];
    let actor = contract.uses[0].actor;
    let spec = graph.pistons[actor.index()];
    let head = SiteId::new(graph.sites.iter().position(|site| site.source == SiteSource::Contact(spec.near)).unwrap() as u32);
    let reset = contract.uses[0].reset.observer().unwrap();
    let comparator = NodeId::new(graph.nodes.len() as u32);
    graph.nodes.push(CircuitNodeDraft {
        position: Some(ORIGIN + BlockPos::new(-8, 0, 0)),
        raw_state: Some(
            Block::RedstoneComparator {
                comparator: RedstoneComparator::new(BlockDirection::South, ComparatorMode::Compare, false),
            }
            .get_id(),
        ),
        kind: CircuitNodeKind::Electrical(NodeType::Comparator {
            mode: ComparatorMode::Compare,
            far_input: None,
            facing_diode: false,
        }),
        state: ComponentState::Electrical(NodeState::simple(false)),
        output_route: None,
    });
    let atom = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::State(StateTest::PistonHeadPresent(actor)));
    let nested = GuardId::new(graph.guards.len() as u32);
    graph.guards.push(Guard::Not(atom));
    let plan = graph.comparator_inputs.len() as u32;
    graph.comparator_inputs.push(ComparatorInputPlan {
        node: comparator,
        rear: head,
        direct_override: Some(ComparatorOverride::LiveComponentOutput(reset)),
        far_override: Some((head, ComparatorOverride::LiveComponentOutput(reset), nested)),
        far_threshold: 15,
    });
    let observer = NodeId::new(graph.nodes.len() as u32);
    let mut outside_observer = graph.nodes[reset.index()].clone();
    outside_observer.position = Some(ORIGIN + BlockPos::new(-7, 0, 0));
    outside_observer.kind = CircuitNodeKind::Observer {
        watched: head,
        facing: BlockFace::Bottom,
    };
    outside_observer.output_route = None;
    graph.nodes.push(outside_observer);
    let catalog = infer_boundaries(graph, &TaskMonitor::default()).unwrap();
    let owner = owner_of_node(graph, &catalog, spec.node).unwrap();
    for role in [
        ComparatorPortRole::Rear,
        ComparatorPortRole::DirectOverride,
        ComparatorPortRole::FarOverrideSite,
        ComparatorPortRole::FarOverrideOutput,
    ] {
        assert!(
            has_port(&catalog, owner, BoundaryDirection::Output, BoundaryPortKind::Comparator { plan, role }),
            "missing comparator role {role:?}"
        );
    }
    assert!(has_port(
        &catalog,
        owner,
        BoundaryDirection::Output,
        BoundaryPortKind::GuardState {
            guard: atom,
            test: StateTest::PistonHeadPresent(actor)
        }
    ));
    assert!(has_port(
        &catalog,
        owner,
        BoundaryDirection::Output,
        BoundaryPortKind::ObserverWatch { observer, site: head }
    ));
    for ordinal in 0..graph.contacts[spec.near.index()].variants.len() {
        assert!(has_port(
            &catalog,
            owner,
            BoundaryDirection::Output,
            BoundaryPortKind::ContactAlternative {
                contact: spec.near,
                ordinal: ordinal as u32
            }
        ));
    }
}

#[test]
fn callback_ordinals_keep_duplicate_updates_and_shape_as_separate_ports() {
    let mut prepared = prepare("adder_1bit");
    let graph = &mut prepared.graph;
    let catalog = graph.plans.instant.boundaries.as_ref().unwrap();
    let original = catalog
        .ports
        .iter()
        .find_map(|port| match port.kind {
            BoundaryPortKind::Callback { occurrence }
                if port.direction == BoundaryDirection::Input
                    && matches!(
                        graph.nodes[graph.routes[occurrence.route.index()].source.index()].kind,
                        CircuitNodeKind::Electrical(NodeType::Lever)
                    ) =>
            {
                Some((port.owner, occurrence))
            }
            _ => None,
        })
        .unwrap();
    let (owner, row) = original;
    let route = &mut graph.routes[row.route.index()];
    let mut deliveries = route.deliveries.to_vec();
    let mut duplicate = deliveries[row.ordinal as usize];
    duplicate.sequence = deliveries.last().unwrap().sequence + 1;
    let duplicate_ordinal = deliveries.len() as u32;
    deliveries.push(duplicate);
    duplicate.sequence += 1;
    duplicate.kind = DeliveryKind::Shape { direction: BlockFace::East };
    let shape_ordinal = deliveries.len() as u32;
    deliveries.push(duplicate);
    route.deliveries = deliveries.into_boxed_slice();
    // This is a read-only scan probe of route semantics, not a lowered executable fixture.
    let catalog = infer_boundaries(graph, &TaskMonitor::default()).unwrap();
    for ordinal in [row.ordinal, duplicate_ordinal, shape_ordinal] {
        assert!(has_port(
            &catalog,
            owner,
            BoundaryDirection::Input,
            BoundaryPortKind::Callback {
                occurrence: RouteOccurrence { route: row.route, ordinal }
            }
        ));
    }
    let shape = RouteOccurrence {
        route: row.route,
        ordinal: shape_ordinal,
    };
    assert!(
        catalog
            .cells
            .iter()
            .flat_map(|cell| &cell.uses)
            .all(|use_| !use_.activations.contains(&shape)),
        "a Shape boundary is not a qualifying electrical piston sample"
    );
}

#[test]
fn constant_conductor_power_is_not_confused_with_fixed_geometry() {
    use crate::redpiler::analysis::instant::boundaries::{bind_owners, scan_boundaries, scan_legacy_boundaries};
    let prepared = prepare("adder_1bit");
    let mut graph = prepared.graph;
    let (wire, site) = graph
        .wires
        .iter()
        .find_map(|wire| {
            wire.canonical_neighbors.iter().find_map(|&site| {
                let SiteSource::Constant(profile) = graph.sites[site.index()].source else {
                    return None;
                };
                let profile = graph.profiles[profile.index()];
                (profile.solid && profile.cube && profile.electrical == ProfileElectrical::None).then_some((wire.node, site))
            })
        })
        .unwrap();
    let mut owners = bind_owners(
        &graph,
        vec![None; graph.pistons.len()],
        vec![None; graph.resources.len()],
        vec![None; graph.contacts.len()],
        &[],
    );
    let binding = BoundaryPortKind::WireNeighborhood { wire, neighbor: site };
    owners.sites[site.index()] = Some(0);
    for scan in [
        scan_boundaries(&graph, &owners, &TaskMonitor::default()).unwrap(),
        scan_legacy_boundaries(&graph, &owners, &TaskMonitor::default()).unwrap(),
    ] {
        assert!(
            scan.ports
                .iter()
                .any(|port| port.owner == 0 && port.direction == BoundaryDirection::Output && port.kind == binding),
            "an outside wire can read power from an owned static conductor"
        );
    }
    owners.sites[site.index()] = None;
    owners.nodes[wire.index()] = Some(0);
    graph
        .power_links
        .retain(|link| !matches!(link.to,PowerTarget::Conductor{site:target,..} if target==site));
    let static_scan = scan_boundaries(&graph, &owners, &TaskMonitor::default()).unwrap();
    assert!(
        !static_scan
            .ports
            .iter()
            .any(|port| port.owner == 0 && port.direction == BoundaryDirection::Input && port.kind == binding),
        "unchanging geometry alone is a compile fact"
    );
    let source=SiteId::new(graph.sites.iter().position(|site|matches!(site.source,SiteSource::Component(node) if matches!(graph.nodes[node.index()].kind,CircuitNodeKind::Electrical(NodeType::Lever)))).unwrap() as u32);
    let always = GuardId::new(graph.guards.iter().position(|guard| *guard == Guard::Always).unwrap() as u32);
    graph.power_links.push(PowerLink {
        from: source,
        from_port: PowerPort::Output,
        to: PowerTarget::Conductor { site, include_dust: true },
        face: BlockFace::Top,
        attenuation: 0,
        guard: always,
    });
    let dynamic_scan = scan_boundaries(&graph, &owners, &TaskMonitor::default()).unwrap();
    assert!(
        dynamic_scan
            .ports
            .iter()
            .any(|port| port.owner == 0 && port.direction == BoundaryDirection::Input && port.kind == binding),
        "the same fixed cube is a live input when it conducts external power"
    );
}

#[test]
fn note_only_contact_reads_include_all_profile_alternatives() {
    use crate::redpiler::analysis::instant::boundaries::{bind_owners, scan_boundaries};
    use crate::redpiler::components::NodeState;
    use mchprs_blocks::blocks::Instrument;
    let mut graph = prepare("adder_1bit").graph;
    let near = graph.pistons[0].near;
    let site = SiteId::new(graph.sites.iter().position(|site| site.source == SiteSource::Contact(near)).unwrap() as u32);
    // Isolate this prepared contact's note read from unrelated power/wire/callback readers.
    // The probe exercises dependency analysis, not lowering this pruned draft.
    graph.wires.clear();
    graph.wire_relations.clear();
    graph.power_links.clear();
    graph.routes.clear();
    let node = NodeId::new(graph.nodes.len() as u32);
    let block = Block::NoteBlock {
        instrument: Instrument::Harp,
        note: 0,
        powered: false,
    };
    graph.nodes.push(CircuitNodeDraft {
        position: Some(graph.sites[site.index()].position.offset(BlockFace::Top)),
        raw_state: Some(block.get_id()),
        kind: CircuitNodeKind::Electrical(NodeType::NoteBlock {
            instrument: Instrument::Harp,
            note: 0,
        }),
        state: ComponentState::Electrical(NodeState::simple(false)),
        output_route: None,
    });
    let mut contact_owners = vec![None; graph.contacts.len()];
    contact_owners[near.index()] = Some(0);
    let owners = bind_owners(
        &graph,
        vec![None; graph.pistons.len()],
        vec![None; graph.resources.len()],
        contact_owners,
        &[],
    );
    let scan = scan_boundaries(&graph, &owners, &TaskMonitor::default()).unwrap();
    assert!(scan.ports.iter().any(|port| port.owner == 0
        && port.direction == BoundaryDirection::Output
        && port.kind
            == BoundaryPortKind::FixedRead {
                node,
                role: FixedReadRole::NoteSupport,
                site
            }));
    for ordinal in 0..graph.contacts[near.index()].variants.len() {
        assert!(scan.ports.iter().any(|port| port.owner == 0
            && port.direction == BoundaryDirection::Output
            && port.kind
                == BoundaryPortKind::ContactAlternative {
                    contact: near,
                    ordinal: ordinal as u32
                }));
    }
}
