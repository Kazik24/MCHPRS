use super::*;
use crate::redpiler::analysis::callbacks;
use crate::redpiler::program_graph::{
    CallbackEdgeRef, CallbackReceiver, CircuitNodeKind, DeliveryKind, Guard, LinkKind, OperandView,
    Route, SiteSource,
};
use mchprs_blocks::block_entities::MovingPistonEntity;
use mchprs_blocks::blocks::{Block, Lever, LeverFace, RedstoneObserver, TrapdoorHalf};
use mchprs_blocks::blocks::{RedstoneMovingPiston, RedstonePistonHead};

fn prepare_callbacks_for(
    world: &PlotWorld,
) -> (GeometryResult, ElectricalDraft, callbacks::CallbackDraft) {
    let geometry = prepare_geometry_for(world).unwrap();
    let mut electrical = electrical::prepare_electrical(
        world,
        geometry.report.bounds,
        &geometry.draft,
        &Default::default(),
    )
    .unwrap();
    let callbacks =
        callbacks::prepare_callbacks(world, &geometry.draft, &mut electrical, &Default::default())
            .unwrap();
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

fn selected_updates(
    world: &PlotWorld,
    electrical: &ElectricalDraft,
    route: &Route,
) -> Vec<(BlockPos, Option<BlockFace>)> {
    route
        .deliveries
        .iter()
        .filter_map(|delivery| {
            let DeliveryKind::Update { direction } = delivery.kind else {
                return None;
            };
            let site = electrical.sites[delivery.target.index()];
            let selected = match site.source {
                SiteSource::Contact(contact) => electrical.contacts[contact.index()]
                    .variants
                    .iter()
                    .any(|variant| {
                        variant.when == delivery.guard
                            && electrical.profiles[variant.profile.index()].raw_state
                                == world.get_block(site.position).get_id()
                    }),
                _ => true,
            };
            selected.then_some((site.position, direction))
        })
        .collect()
}

fn native_completion_updates(
    world: &mut PlotWorld,
    electrical: &ElectricalDraft,
    route: &Route,
    position: BlockPos,
    piston: RedstonePiston,
    restored: Block,
    source: bool,
    extending: bool,
    interrupted: bool,
) -> Vec<(BlockPos, Option<BlockFace>)> {
    let moving = Block::MovingPiston {
        moving: RedstoneMovingPiston {
            facing: piston.facing,
            sticky: piston.sticky,
        },
    };
    world.set_block(position, moving);
    world.set_block_entity(
        position,
        BlockEntity::MovingPiston(MovingPistonEntity {
            facing: piston.facing.into(),
            source,
            extending,
            block_state: restored.get_id(),
            progress: u8::MAX,
        }),
    );
    let motion = world.piston_motion_index(position, None).unwrap();
    let identity = world.piston_state().motions[motion].identity;
    let watched: Vec<_> = route
        .deliveries
        .iter()
        .map(|delivery| route_position(electrical, delivery))
        .chain(std::iter::once(position))
        .collect();
    let trace = crate::redstone::instant_piston_tests::capture_at(&watched, || {
        if interrupted {
            crate::redstone::piston::finish_interrupted_for_test(world, position);
        } else {
            crate::redstone::piston::tick_motion(world, position, identity);
        }
    });
    trace
        .iter()
        .filter(|entry| {
            entry["kind"] == "callback"
                && entry["data"]["parents"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
        })
        .map(|entry| {
            (
                serde_json::from_value(entry["data"]["pos"].clone()).unwrap(),
                serde_json::from_value(entry["data"]["dir"].clone()).unwrap(),
            )
        })
        .collect()
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
fn piston_extension_far_route_contains_only_shape_notifications() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::East, true, false));
    world.set_block(BASE.offset(BlockFace::East), Block::RedstoneBlock {});
    let (_, electrical, callbacks) = prepare_callbacks_for(&world);
    let piston = callbacks.pistons[0];
    let shape_route = &callbacks.routes[piston.routes.far_shape_changed.unwrap().index()];
    let notify_route = &callbacks.routes[piston.routes.far_changed.unwrap().index()];

    assert!(!shape_route.deliveries.is_empty());
    assert!(
        shape_route
            .deliveries
            .iter()
            .all(|delivery| matches!(delivery.kind, DeliveryKind::Shape { .. }))
    );
    assert!(
        notify_route
            .deliveries
            .iter()
            .any(|delivery| matches!(delivery.kind, DeliveryKind::Update { .. }))
    );
    assert!(
        electrical
            .sites
            .iter()
            .any(|site| site.position == BASE.offset(BlockFace::East).offset(BlockFace::East))
    );
}

#[test]
fn restoration_routes_match_native_head_and_resource_completions() {
    let mut head_world = empty();
    head_world.set_block(BASE, piston_block(BlockFace::East, false, false));
    head_world.set_block(BASE.offset(BlockFace::Bottom), Block::Stone {});
    let (head_geometry, head_electrical, head_callbacks) = prepare_callbacks_for(&head_world);
    let head_piston = head_callbacks.pistons[0];
    let head_route = &head_callbacks.routes[head_piston.routes.head_restored.index()];
    let head_pos = head_geometry.draft.contacts[head_piston.near.index()].position;
    let normal = RedstonePiston {
        facing: mchprs_blocks::BlockFacing::East,
        sticky: false,
        extended: true,
    };
    head_world.set_block(BASE, Block::Piston { piston: normal });
    let head_actual = native_completion_updates(
        &mut head_world,
        &head_electrical,
        head_route,
        head_pos,
        normal,
        Block::PistonHead {
            head: RedstonePistonHead::from(normal),
        },
        true,
        true,
        false,
    );
    let head_expected = selected_updates(&head_world, &head_electrical, head_route);
    assert_eq!(head_actual, head_expected);
    assert!(!head_actual.contains(&(head_pos, None)));

    let make_resource_world = || {
        let mut world = empty();
        world.set_block(BASE, piston_block(BlockFace::East, true, false));
        world.set_block(BASE.offset(BlockFace::Bottom), Block::Stone {});
        world.set_block(BASE.offset(BlockFace::East), Block::RedstoneBlock {});
        world
    };
    let entry = make_resource_world();
    let (geometry, electrical, callbacks) = prepare_callbacks_for(&entry);
    let piston = callbacks.pistons[0];
    let near_pos = geometry.draft.contacts[piston.near.index()].position;
    let far_contact = match piston.lane {
        crate::redpiler::program_graph::PistonLane::SingleResource { far } => far,
        crate::redpiler::program_graph::PistonLane::Empty => panic!("expected resource lane"),
    };
    let far_pos = geometry.draft.contacts[far_contact.index()].position;
    let near_route_id = piston.routes.resource_restored_near.unwrap();
    let far_route_id = piston.routes.resource_restored_far.unwrap();
    let near_route = &callbacks.routes[near_route_id.index()];
    let far_route = &callbacks.routes[far_route_id.index()];
    let sticky = RedstonePiston {
        facing: mchprs_blocks::BlockFacing::East,
        sticky: true,
        extended: false,
    };

    let mut extension = make_resource_world();
    extension.set_block(
        BASE,
        Block::Piston {
            piston: sticky.extend(true),
        },
    );
    extension.set_block(
        near_pos,
        Block::PistonHead {
            head: RedstonePistonHead::from(sticky.extend(true)),
        },
    );
    let extension_actual = native_completion_updates(
        &mut extension,
        &electrical,
        far_route,
        far_pos,
        sticky,
        Block::RedstoneBlock {},
        false,
        true,
        false,
    );
    assert_eq!(
        extension_actual,
        selected_updates(&extension, &electrical, far_route)
    );
    assert_eq!(extension_actual.first(), Some(&(far_pos, None)));

    let mut pull = empty();
    pull.set_block(BASE, piston_block(BlockFace::East, true, false));
    pull.set_block(BASE.offset(BlockFace::Bottom), Block::Stone {});
    let pull_actual = native_completion_updates(
        &mut pull,
        &electrical,
        near_route,
        near_pos,
        sticky,
        Block::RedstoneBlock {},
        false,
        false,
        false,
    );
    assert_eq!(
        pull_actual,
        selected_updates(&pull, &electrical, near_route)
    );
    assert_eq!(pull_actual.first(), Some(&(near_pos, None)));

    let mut interrupted = make_resource_world();
    interrupted.set_block(
        BASE,
        Block::Piston {
            piston: sticky.extend(true),
        },
    );
    interrupted.set_block(
        near_pos,
        Block::PistonHead {
            head: RedstonePistonHead::from(sticky.extend(true)),
        },
    );
    let interrupted_actual = native_completion_updates(
        &mut interrupted,
        &electrical,
        far_route,
        far_pos,
        sticky,
        Block::RedstoneBlock {},
        false,
        true,
        true,
    );
    assert_eq!(
        interrupted_actual,
        selected_updates(&interrupted, &electrical, far_route)
    );
    assert_eq!(interrupted_actual.first(), Some(&(far_pos, None)));
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
    let mut electrical =
        electrical::prepare_electrical(&world, bounds, &geometry.draft, &Default::default())
            .unwrap();
    let error = callbacks::prepare_callbacks(
        &world,
        &geometry.draft,
        &mut electrical,
        &Default::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        callbacks::CallbackError::UninventoriedReceiver {
            source: BASE,
            target: external,
            block: repeater.get_id(),
        }
    );
}

#[test]
fn unsupported_external_hopper_and_trapdoor_dependencies_name_type_and_reason() {
    for (block, expected_name) in [
        (Block::from_name("hopper").unwrap(), "hopper"),
        (
            Block::IronTrapdoor {
                facing: BlockDirection::North,
                half: TrapdoorHalf::Bottom,
                powered: false,
            },
            "iron_trapdoor",
        ),
    ] {
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
        world.set_block(external, block);
        let geometry = prepare_geometry_with_bounds(&world, (BASE, BASE)).unwrap();
        let mut electrical = electrical::prepare_electrical(
            &world,
            (BASE, BASE),
            &geometry.draft,
            &Default::default(),
        )
        .unwrap();
        let error = callbacks::prepare_callbacks(
            &world,
            &geometry.draft,
            &mut electrical,
            &Default::default(),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains(expected_name), "{message}");
        assert!(message.contains(&format!("{external:?}")), "{message}");
        assert!(message.contains("not implemented in Graph"), "{message}");
    }
}
