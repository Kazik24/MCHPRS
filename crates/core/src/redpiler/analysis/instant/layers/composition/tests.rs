use super::*;
use crate::redpiler::analysis::instant::layers::{reduction::reduce_roots, tests::empty_graph};
use mchprs_blocks::{BlockPos, blocks::Block};

fn occurrence(actor: u32) -> RouteOccurrence {
    RouteOccurrence {
        route: RouteId::new(actor),
        ordinal: 0,
    }
}

fn cell(actor: u32, resource: u32, power: Option<PowerLink>) -> CellContract {
    let actor = PistonId::new(actor);
    let mut pins = vec![
        CellPin::Read(CellRead::State(StateTest::PistonAvailability {
            piston: actor,
            is: PistonAvailability::Ready,
        })),
        CellPin::Read(CellRead::State(StateTest::PistonExtended(actor))),
        CellPin::Read(CellRead::Activation(actor)),
        CellPin::Read(CellRead::State(StateTest::ResourceEnabled(ResourceId::new(resource)))),
        CellPin::Read(CellRead::State(StateTest::ResourceAt {
            resource: ResourceId::new(resource),
            contact: ContactId::new(0),
        })),
        CellPin::Gate {
            kind: CellGateKind::All,
            inputs: vec![CellPinId(3), CellPinId(4)].into(),
        },
    ];
    pins.push(match power {
        Some(link) => CellPin::Read(CellRead::Contribution(link)),
        None => CellPin::Gate {
            kind: CellGateKind::Any,
            inputs: Box::new([]),
        },
    });
    pins.push(CellPin::Gate {
        kind: CellGateKind::Invert,
        inputs: vec![CellPinId(6)].into(),
    });
    pins.push(CellPin::Gate {
        kind: CellGateKind::All,
        inputs: vec![CellPinId(0), CellPinId(1), CellPinId(2), CellPinId(7)].into(),
    });
    pins.push(CellPin::Gate {
        kind: CellGateKind::All,
        inputs: vec![CellPinId(0), CellPinId(1), CellPinId(7)].into(),
    });
    let mut use_ = super::super::tests::use_with_activations(vec![occurrence(actor.index() as u32)].into(), RouteId::new(0), RouteId::new(0), None);
    use_.actor = actor;
    use_.requested = CellPinId(8);
    use_.valid = CellPinId(9);
    use_.powered = CellPinId(6);
    use_.retraction.transfer = Some(CellResourceTransfer {
        resource: ResourceId::new(resource),
        source: ContactId::new(0),
        destination: ContactId::new(actor.index() as u32 + 1),
        resource_revision_increment: 1,
        when_at_source: CellPinId(5),
    });
    CellContract {
        uses: vec![use_].into(),
        pins: pins.into(),
        stationary_entry: CellPinId(0),
        resources: Box::new([]),
    }
}

fn fixture(shared: bool) -> (CompileGraph, BoundaryCatalog, RegionComponent) {
    let link = PowerLink {
        from: SiteId::new(0),
        from_port: PowerPort::Output,
        to: PowerTarget::Input {
            node: NodeId::new(0),
            port: InputPort::Main,
        },
        face: BlockFace::West,
        attenuation: 2,
        guard: GuardId::new(0),
    };
    let mut graph = empty_graph(link);
    graph.power_links.clear();
    graph.resources = (0..2)
        .map(|_| ResourceSpec {
            material: ProfileId::new(0),
            contacts: vec![ContactId::new(0), ContactId::new(1), ContactId::new(2)].into(),
        })
        .collect();
    graph.guards.push(Guard::State(StateTest::ResourceEnabled(ResourceId::new(0))));
    graph.guards.push(Guard::Not(GuardId::new(1)));
    let pos = BlockPos::new(0, 20, 0);
    graph.sites.push(SiteSpec {
        position: pos,
        source: SiteSource::Contact(ContactId::new(0)),
        source_strength: 0,
    });
    graph.profiles = [Block::RedstoneBlock, Block::Air]
        .into_iter()
        .map(|block| Profile {
            raw_state: block.get_id(),
            registry_type: 0,
            solid: block.is_solid(),
            transparent: block.is_transparent(),
            cube: block.is_cube(),
            electrical: ProfileElectrical::None,
        })
        .collect();
    graph.contacts.push(ContactSpec {
        node: NodeId::new(0),
        position: pos,
        variants: vec![
            ContactVariant {
                when: GuardId::new(1),
                profile: ProfileId::new(0),
                receiver: None,
            },
            ContactVariant {
                when: GuardId::new(2),
                profile: ProfileId::new(1),
                receiver: None,
            },
        ]
        .into(),
    });
    let catalog = BoundaryCatalog {
        cells: vec![
            cell(0, 0, None),
            cell(1, if shared { 0 } else { 1 }, if shared { None } else { Some(link) }),
        ]
        .into(),
        ..Default::default()
    };
    let component = RegionComponent {
        cells: vec![0, 1].into(),
        corridors: Box::new([]),
        ports: Box::new([]),
    };
    (graph, catalog, component)
}

fn evaluated(composition: &Composition<'_>, roots: &[LayerExprId]) -> Vec<bool> {
    let roots = roots.iter().enumerate().map(|(i, &root)| (LayerRoot::Output(i), root)).collect();
    let program = reduce_roots(
        &composition.builder.inputs,
        &composition.builder.expressions,
        roots,
        &TaskMonitor::default(),
    )
    .unwrap();
    let mut values = vec![false; program.expressions.len()];
    program
        .evaluate(&mut values, |input| match input.source {
            LayerInputSource::State(StateTest::PistonExtended(_) | StateTest::PistonHeadPresent(_) | StateTest::ResourceEnabled(_)) => true,
            LayerInputSource::State(StateTest::ResourceAt { contact, .. }) => contact == ContactId::new(0),
            LayerInputSource::State(StateTest::PistonAvailability { is, .. }) => is == PistonAvailability::Ready,
            other => panic!("unexpected input {other:?}"),
        })
        .unwrap();
    program.roots.iter().map(|(_, root)| values[root.0 as usize]).collect()
}

#[test]
fn failed_samples_can_retry_after_a_causal_resource_change_without_burning_the_use() {
    let (graph, catalog, component) = fixture(false);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut composition = Composition::new(&graph, &catalog, &power, &component, &monitor).unwrap();
    let yes = composition.builder.constant(true).unwrap();
    composition.sample(0, PistonId::new(0), yes, vec![occurrence(0)].into()).unwrap();
    composition.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    let first = composition.begin_next().unwrap().unwrap();
    // The base phase still observes the old source; Far is the first cut with its absence.
    composition.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    assert!(composition.begin_next().is_err());
    composition.finish_base(first).unwrap();
    composition.sample(1, PistonId::new(1), yes, vec![occurrence(1)].into()).unwrap();
    while let Some(next) = composition.begin_next().unwrap() {
        composition.finish_base(next).unwrap();
    }
    let accepted: Vec<_> = composition.layers.iter().map(|layer| layer.uses[0].accepted).collect();
    assert_eq!(evaluated(&composition, &accepted), vec![true, false, false, true]);
    assert_eq!(
        evaluated(&composition, &composition.resources.iter().map(|r| r.transfer_when).collect::<Vec<_>>()),
        vec![true, false, false, true]
    );
}

#[test]
fn shared_resource_has_one_fifo_winner_and_reversing_requests_changes_the_destination() {
    for order in [[0, 1], [1, 0]] {
        let (graph, catalog, component) = fixture(true);
        let monitor = TaskMonitor::default();
        let power = index_power(&graph);
        let mut composition = Composition::new(&graph, &catalog, &power, &component, &monitor).unwrap();
        let yes = composition.builder.constant(true).unwrap();
        for actor in order {
            composition
                .sample(actor, PistonId::new(actor), yes, vec![occurrence(actor)].into())
                .unwrap();
        }
        while let Some(next) = composition.begin_next().unwrap() {
            composition.finish_base(next).unwrap();
        }
        assert_eq!(
            evaluated(
                &composition,
                &composition.layers.iter().map(|layer| layer.uses[0].accepted).collect::<Vec<_>>()
            ),
            vec![true, true]
        );
        assert_eq!(
            evaluated(&composition, &composition.resources.iter().map(|r| r.transfer_when).collect::<Vec<_>>()),
            vec![true, false]
        );
        assert_eq!(composition.resources[0].destination, ContactId::new(order[0] + 1));
    }
}

#[test]
fn queued_duplicates_and_feedback_cannot_accept_an_actor_twice() {
    let (graph, catalog, component) = fixture(true);
    let monitor = TaskMonitor::default();
    let power = index_power(&graph);
    let mut composition = Composition::new(&graph, &catalog, &power, &component, &monitor).unwrap();
    let no = composition.builder.constant(false).unwrap();
    let yes = composition.builder.constant(true).unwrap();
    for event in [no, yes, yes] {
        composition.sample(0, PistonId::new(0), event, vec![occurrence(0)].into()).unwrap();
    }
    while let Some(next) = composition.begin_next().unwrap() {
        composition.finish_base(next).unwrap();
    }
    composition.sample(0, PistonId::new(0), yes, vec![occurrence(0)].into()).unwrap();
    let next = composition.begin_next().unwrap().unwrap();
    composition.finish_base(next).unwrap();
    assert_eq!(
        evaluated(
            &composition,
            &composition.layers.iter().map(|layer| layer.uses[0].accepted).collect::<Vec<_>>()
        ),
        vec![false, true, false, false]
    );
}
