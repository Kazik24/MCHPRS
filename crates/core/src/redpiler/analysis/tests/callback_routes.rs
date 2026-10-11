use super::*;
use crate::redpiler::analysis::callbacks;
use crate::redpiler::program_graph::{
    CallbackEdgeRef, CallbackReceiver, CircuitNodeKind, DeliveryKind, Guard, LinkKind, OperandView,
    SiteSource,
};
use mchprs_blocks::blocks::{Block, Lever, LeverFace, RedstoneObserver};

fn prepare_callbacks_for(
    world: &PlotWorld,
) -> (GeometryResult, ElectricalDraft, callbacks::CallbackDraft) {
    let geometry = prepare_geometry_for(world).unwrap();
    let mut electrical =
        electrical::prepare_electrical(world, geometry.report.bounds, &geometry.draft).unwrap();
    let callbacks = callbacks::prepare_callbacks(world, &geometry.draft, &mut electrical).unwrap();
    (geometry, electrical, callbacks)
}

fn route_for(
    electrical: &ElectricalDraft,
    position: BlockPos,
    matches: impl Fn(&CircuitNodeKind) -> bool,
) -> crate::redpiler::program_graph::RouteId {
    let node = electrical
        .nodes
        .iter()
        .find(|node| node.position == Some(position) && matches(&node.kind))
        .unwrap_or_else(|| panic!("no callback source at {position:?}"));
    node.output_route
        .unwrap_or_else(|| panic!("node at {position:?} has no output route"))
}

fn route_position(
    electrical: &ElectricalDraft,
    delivery: &crate::redpiler::program_graph::RouteDelivery,
) -> BlockPos {
    electrical.sites[delivery.target.index()].position
}

#[test]
fn control_routes_keep_native_sequence_directions_and_repeated_targets() {
    let mut world = empty();
    world.set_block(
        BASE,
        Block::Lever {
            lever: Lever::new(
                LeverFace::Floor,
                mchprs_blocks::BlockDirection::North,
                false,
            ),
        },
    );
    world.set_block(BASE.offset(BlockFace::Bottom), Block::Stone {});
    let (_, electrical, callbacks) = prepare_callbacks_for(&world);
    let route_id = route_for(&electrical, BASE, |kind| {
        matches!(kind, CircuitNodeKind::Electrical(NodeType::Lever))
    });
    let route = &callbacks.routes[route_id.index()];

    let mut expected = Vec::new();
    for center in [BASE, BASE.offset(BlockFace::Bottom)] {
        crate::redstone::surrounding_notifications(center, |target, direction, _| {
            expected.push((target, Some(direction)));
        });
    }
    assert_eq!(route.deliveries.len(), expected.len());
    for (sequence, (delivery, (target, direction))) in
        route.deliveries.iter().zip(expected).enumerate()
    {
        assert_eq!(delivery.sequence, sequence as u32);
        assert_eq!(route_position(&electrical, delivery), target);
        assert_eq!(delivery.kind, DeliveryKind::Update { direction });
    }
    let repeated = route
        .deliveries
        .iter()
        .filter(|delivery| route_position(&electrical, delivery) == BASE)
        .count();
    assert!(
        repeated > 1,
        "overlapping notification loops must retain duplicates"
    );
}

#[test]
fn repeater_and_comparator_keep_their_distinct_first_directions() {
    let mut world = empty();
    let repeater_pos = BASE;
    let comparator_pos = BASE + BlockPos::new(0, 0, 4);
    world.set_block(
        repeater_pos,
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                facing: mchprs_blocks::BlockDirection::East,
                ..Default::default()
            },
        },
    );
    world.set_block(
        comparator_pos,
        Block::RedstoneComparator {
            comparator: RedstoneComparator::new(
                mchprs_blocks::BlockDirection::East,
                ComparatorMode::Compare,
                false,
            ),
        },
    );
    let (_, electrical, callbacks) = prepare_callbacks_for(&world);
    let repeater_route = route_for(&electrical, repeater_pos, |kind| {
        matches!(kind, CircuitNodeKind::Electrical(NodeType::Repeater { .. }))
    });
    let comparator_route = route_for(&electrical, comparator_pos, |kind| {
        matches!(
            kind,
            CircuitNodeKind::Electrical(NodeType::Comparator { .. })
        )
    });
    let first = |route_id: crate::redpiler::program_graph::RouteId| {
        callbacks.routes[route_id.index()].deliveries[0]
    };
    assert_eq!(
        first(repeater_route).kind,
        DeliveryKind::Update {
            direction: Some(BlockFace::East)
        }
    );
    assert_eq!(
        first(comparator_route).kind,
        DeliveryKind::Update {
            direction: Some(BlockFace::West)
        }
    );
    let route = &callbacks.routes[comparator_route.index()];
    assert_eq!(route.deliveries.len(), 7);

    // Compare mode notifies even when its stored analog output remains zero.
    world.set_block_entity(
        comparator_pos,
        BlockEntity::Comparator { output_strength: 0 },
    );
    let front = comparator_pos.offset(BlockFace::West);
    let watched: Vec<_> = std::iter::once(front)
        .chain(BlockFace::values().map(|face| front.offset(face)))
        .collect();
    let callbacks_seen = crate::redstone::instant_piston_tests::capture_at(&watched, || {
        let Block::RedstoneComparator { comparator } = world.get_block(comparator_pos) else {
            unreachable!();
        };
        crate::redstone::comparator::tick(comparator, &mut world, comparator_pos);
    });
    let updates: Vec<_> = callbacks_seen
        .iter()
        .filter(|entry| entry["kind"] == "callback")
        .map(|entry| (entry["data"]["pos"].clone(), entry["data"]["dir"].clone()))
        .collect();
    assert_eq!(updates.len(), route.deliveries.len());
    assert_eq!(updates[0].1, serde_json::json!(BlockFace::West));

    for (index, delivery) in route.deliveries.iter().enumerate() {
        let route_ref = CallbackEdgeRef {
            route: comparator_route,
            sequence: delivery.sequence,
        };
        if let Some(receiver) = delivery.receiver {
            let to = match receiver {
                CallbackReceiver::Component(node) => node,
                _ => continue,
            };
            assert!(
                callbacks.links.iter().any(|link| {
                    link.callback == Some(route_ref)
                        && link.from == route.source
                        && link.to == to
                        && link.kind
                            == LinkKind::Update {
                                direction: match delivery.kind {
                                    DeliveryKind::Update { direction } => direction,
                                    DeliveryKind::Shape { .. } => unreachable!(),
                                },
                                guard: delivery.guard,
                            }
                }),
                "route delivery {index} must have its typed callback edge"
            );
        }
    }
}

#[test]
fn piston_route_separates_shape_observer_and_power_notifications() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::East, false, false));
    let observer_pos = BASE.offset(BlockFace::West);
    world.set_block(
        observer_pos,
        Block::Observer {
            observer: RedstoneObserver {
                facing: mchprs_blocks::BlockFacing::East,
                powered: false,
            },
        },
    );
    let (geometry, electrical, callbacks) = prepare_callbacks_for(&world);
    let piston = &callbacks.pistons[0];
    let route_id = piston.routes.base_changed;
    let route = &callbacks.routes[route_id.index()];
    let observer_deliveries: Vec<_> = route
        .deliveries
        .iter()
        .filter(|delivery| route_position(&electrical, delivery) == observer_pos)
        .collect();
    assert_eq!(observer_deliveries.len(), 2);
    assert_eq!(
        observer_deliveries[0].kind,
        DeliveryKind::Shape {
            direction: BlockFace::West
        }
    );
    assert_eq!(
        observer_deliveries[1].kind,
        DeliveryKind::Update {
            direction: Some(BlockFace::East)
        }
    );
    assert!(route.deliveries.iter().all(|delivery| {
        route_position(&electrical, delivery) != observer_pos
            || !matches!(
                delivery.kind,
                DeliveryKind::Update {
                    direction: Some(BlockFace::West)
                }
            )
    }));

    let observer = electrical
        .nodes
        .iter()
        .find(|node| node.position == Some(observer_pos))
        .unwrap();
    let CircuitNodeKind::Observer { watched, .. } = &observer.kind else {
        panic!("expected specialized observer")
    };
    assert_eq!(electrical.sites[watched.index()].position, BASE);
    assert!(matches!(
        electrical.sites[watched.index()].source,
        SiteSource::Contact(_)
    ));

    let near_pos = geometry.draft.contacts[piston.near.index()].position;
    assert!(
        route.deliveries.iter().any(|delivery| {
            route_position(&electrical, delivery) == near_pos
                && delivery.receiver
                    == Some(CallbackReceiver::PistonHead(geometry.draft.pistons[0].id))
        }),
        "the prospective head is selected by the near contact at delivery"
    );
}

#[test]
fn turbo_route_retains_captured_contact_eligibility() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::East, false, false));
    let wire_pos = BASE + BlockPos::new(1, 0, -1);
    world.set_block(wire_pos.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(
        wire_pos,
        Block::RedstoneWire {
            wire: Default::default(),
        },
    );
    let (geometry, electrical, callbacks) = prepare_callbacks_for(&world);
    let wire_index = electrical
        .wires
        .iter()
        .position(|wire| electrical.nodes[wire.node.index()].position == Some(wire_pos))
        .unwrap();
    let near_pos = geometry.draft.pistons[0].near;
    let near_pos = geometry.draft.contacts[near_pos.index()].position;
    let mut found_captured_head = false;
    let route = electrical.wires[wire_index].turbo_route.unwrap();
    for delivery in callbacks.routes[route.index()].deliveries.iter() {
        if route_position(&electrical, delivery) == near_pos
            && delivery.receiver == Some(CallbackReceiver::PistonHead(geometry.draft.pistons[0].id))
        {
            assert_eq!(delivery.view, OperandView::Captured);
            assert!(matches!(
                electrical.guards[delivery.guard.index()],
                Guard::Site {
                    site,
                    view: OperandView::Captured,
                    ..
                } if site == delivery.target
            ));
            found_captured_head = true;
        }
    }
    assert!(
        found_captured_head,
        "wire Turbo candidates must retain captured head eligibility"
    );
}

#[test]
fn callback_targets_outside_component_inventory_are_rejected() {
    let mut world = empty();
    world.set_block(
        BASE,
        Block::Lever {
            lever: Lever::new(
                LeverFace::Floor,
                mchprs_blocks::BlockDirection::North,
                false,
            ),
        },
    );
    let external = BASE.offset(BlockFace::East);
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater::default(),
    };
    world.set_block(external, repeater);
    let bounds = (BASE, BASE);
    let geometry = prepare_geometry_with_bounds(&world, bounds).unwrap();
    let mut electrical = electrical::prepare_electrical(&world, bounds, &geometry.draft).unwrap();
    let error = callbacks::prepare_callbacks(&world, &geometry.draft, &mut electrical).unwrap_err();
    assert_eq!(
        error,
        callbacks::CallbackError::UninventoriedReceiver {
            source: BASE,
            target: external,
            block: repeater.get_id(),
        }
    );
}
