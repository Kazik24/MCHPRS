//! Independent query witnesses and complete-stage parser checks; this is not an executor.
use super::*;
use crate::redpiler::analysis::callbacks;
use crate::redpiler::program_graph::{
    CircuitDraft, ContactId, DeliveryKind, NodeId, OperandView, PistonAvailability, PistonState,
    SiteId, SiteTest,
};
use mchprs_blocks::blocks::{Lever, LeverFace, RedstoneObserver};

fn guard_holds(
    world: &PlotWorld,
    draft: &ElectricalDraft,
    guard: GuardId,
    pistons: &[PistonState],
    resources: &[ResourceState],
) -> bool {
    match &draft.guards[guard.index()] {
        Guard::Always => true,
        Guard::Not(child) => !guard_holds(world, draft, *child, pistons, resources),
        Guard::All(children) => children
            .iter()
            .all(|child| guard_holds(world, draft, *child, pistons, resources)),
        Guard::Any(children) => children
            .iter()
            .any(|child| guard_holds(world, draft, *child, pistons, resources)),
        Guard::State(StateTest::PistonExtended(id)) => pistons[id.index()].extended,
        Guard::State(StateTest::PistonAvailability { piston, is }) => {
            pistons[piston.index()].availability == *is
        }
        Guard::State(StateTest::ResourceAt { resource, contact }) => {
            resources[resource.index()].at == *contact
        }
        Guard::State(StateTest::ResourceEnabled(id)) => resources[id.index()].enabled,
        Guard::Site { site, view, test } => {
            assert_eq!(
                *view,
                OperandView::Live,
                "query test requires live operands"
            );
            let block = world.get_block(draft.sites[site.index()].position);
            match test {
                SiteTest::Solid => block.is_solid(),
                SiteTest::Transparent => block.is_transparent(),
                SiteTest::Cube => block.is_cube(),
                SiteTest::IsWire => matches!(block, Block::RedstoneWire { .. }),
                SiteTest::Profile(id) => block.get_id() == draft.profiles[id.index()].raw_state,
            }
        }
    }
}

fn channel_strength(
    world: &PlotWorld,
    draft: &ElectricalDraft,
    site: SiteId,
    port: PowerPort,
    pistons: &[PistonState],
    resources: &[ResourceState],
) -> u8 {
    if let PowerPort::Conducted { include_dust } = port {
        return draft
            .links
            .iter()
            .filter(|link| {
                link.to == ElectricalTarget::Conductor { site, include_dust }
                    && guard_holds(world, draft, link.guard, pistons, resources)
            })
            .map(|link| {
                channel_strength(world, draft, link.from, link.from_port, pistons, resources)
                    .saturating_sub(link.attenuation)
            })
            .max()
            .unwrap_or(0);
    }
    let position = draft.sites[site.index()].position;
    let block = world.get_block(position);
    let strength = crate::redstone::source_strength(block, world, position);
    let emits = match port {
        PowerPort::Stored | PowerPort::Output => true,
        PowerPort::Weak(face) => {
            crate::redstone::power::emits_weak_power(block, world, position, face, true)
        }
        PowerPort::Strong(face) => {
            crate::redstone::power::emits_strong_power(block, world, position, face, true)
        }
        PowerPort::Conducted { .. } => unreachable!(),
    };
    if emits { strength } else { 0 }
}

fn input_strength(
    world: &PlotWorld,
    draft: &ElectricalDraft,
    node: NodeId,
    port: InputPort,
    pistons: &[PistonState],
    resources: &[ResourceState],
) -> u8 {
    draft
        .links
        .iter()
        .filter(|link| {
            link.to == ElectricalTarget::Input { node, port }
                && guard_holds(world, draft, link.guard, pistons, resources)
        })
        .map(|link| {
            channel_strength(world, draft, link.from, link.from_port, pistons, resources)
                .saturating_sub(link.attenuation)
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn directional_lamp_queries_match_interpreter_in_every_face() {
    let sources = [
        Block::RedstoneBlock,
        Block::RedstoneTorch { lit: true },
        Block::RedstoneRepeater {
            repeater: RedstoneRepeater {
                powered: true,
                ..Default::default()
            },
        },
        Block::Observer {
            observer: RedstoneObserver {
                powered: true,
                ..Default::default()
            },
        },
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, true),
        },
    ];
    for source in sources {
        for face in BlockFace::values() {
            for through_cube in [false, true] {
                let mut world = empty();
                world.set_block(BASE, Block::RedstoneLamp { lit: false });
                let adjacent = BASE.offset(face);
                let source_pos = if through_cube {
                    world.set_block(adjacent, Block::Stone {});
                    adjacent.offset(face)
                } else {
                    adjacent
                };
                world.set_block(source_pos, source);
                let (geometry, draft) = prepare_electrical_for(&world);
                let node = lamp_node_at(&draft, BASE);
                let actual = input_strength(
                    &world,
                    &draft,
                    node,
                    InputPort::Main,
                    &geometry.draft.initial_pistons,
                    &geometry.draft.initial_resources,
                );
                let expected = crate::redstone::get_redstone_power(
                    world.get_block(adjacent),
                    &world,
                    adjacent,
                    face,
                );
                assert_eq!(
                    actual, expected,
                    "{source:?}, {face:?}, cube={through_cube}"
                );
                assert_eq!(
                    actual > 0,
                    crate::redstone::redstone_lamp_should_be_lit(&world, BASE)
                );
            }
        }
    }
}

#[test]
fn qc_sampling_matches_native_in_all_orientations_without_routes() {
    for facing in BlockFace::values() {
        let mut world = empty();
        world.set_block(BASE, piston_block(facing, false, false));
        let (geometry, draft) = prepare_electrical_for(&world);
        let node = geometry.draft.pistons[0].node;
        let candidates = BlockFace::values()
            .map(|face| BASE.offset(face))
            .into_iter()
            .chain(BlockFace::values().map(|face| BASE.offset(BlockFace::Top).offset(face)));
        for source in candidates {
            if source == BASE || source == BASE.offset(facing) {
                continue;
            }
            world.set_block(source, Block::RedstoneBlock);
            // Rebuild once per candidate so source discovery cannot rely on the previous source.
            let (geometry, draft) = prepare_electrical_for(&world);
            let sampled = [InputPort::Main, InputPort::Quasi].into_iter().any(|port| {
                input_strength(
                    &world,
                    &draft,
                    node,
                    port,
                    &geometry.draft.initial_pistons,
                    &geometry.draft.initial_resources,
                ) > 0
            });
            assert_eq!(
                sampled,
                crate::redstone::piston::should_piston_extend(&world, facing.into(), BASE),
                "facing={facing:?}, source={source:?}"
            );
            assert!(draft.nodes.iter().all(|node| node.output_route.is_none()));
            world.set_block(source, Block::Air);
        }
        assert!(!draft.links.iter().any(|link| link.to
            == ElectricalTarget::Input {
                node,
                port: InputPort::Main
            }));
    }
}

#[test]
fn single_resource_contact_guards_partition_legal_stationary_states() {
    for facing in BlockFace::values() {
        let mut world = empty();
        world.set_block(BASE, piston_block(facing, true, false));
        let near = BASE.offset(facing);
        let far = near.offset(facing);
        world.set_block(near, Block::RedstoneBlock);
        let (geometry, draft) = prepare_electrical_for(&world);
        for extended in [false, true] {
            let mut pistons = geometry.draft.initial_pistons.clone();
            let mut resources = geometry.draft.initial_resources.clone();
            pistons[0].extended = extended;
            let resource_pos = if extended { far } else { near };
            resources[0].at = ContactId::new(
                geometry
                    .draft
                    .contacts
                    .iter()
                    .position(|contact| contact.position == resource_pos)
                    .unwrap() as u32,
            );
            for contact in &draft.contacts {
                let selected: Vec<_> = contact
                    .variants
                    .iter()
                    .filter(|variant| {
                        guard_holds(&world, &draft, variant.when, &pistons, &resources)
                    })
                    .collect();
                assert_eq!(
                    selected.len(),
                    1,
                    "{facing:?}, extended={extended}, contact={:?}",
                    contact.position
                );
            }
        }
        for availability in [
            PistonAvailability::HeadPending,
            PistonAvailability::PublishingHeadRemoval,
            PistonAvailability::BasePending,
        ] {
            let mut pistons = geometry.draft.initial_pistons.clone();
            pistons[0].extended = true;
            pistons[0].availability = availability;
            let mut resources = geometry.draft.initial_resources.clone();
            resources[0].at = geometry.draft.pistons[0].far;
            resources[0].enabled = false;
            for contact in &draft.contacts {
                assert_eq!(
                    contact
                        .variants
                        .iter()
                        .filter(|variant| {
                            guard_holds(&world, &draft, variant.when, &pistons, &resources)
                        })
                        .count(),
                    1,
                    "{facing:?}, availability={availability:?}, contact={:?}",
                    contact.position
                );
            }
        }
    }
}

#[test]
fn restored_head_has_no_native_excluded_self_update() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::East, false, false));
    let geometry = prepare_geometry_for(&world).unwrap();
    let mut electrical = electrical::prepare_electrical(
        &world,
        geometry.report.bounds,
        &geometry.draft,
        &Default::default(),
    )
    .unwrap();
    let callbacks = callbacks::prepare_callbacks(
        &world,
        &geometry.draft,
        &mut electrical,
        &Default::default(),
    )
    .unwrap();
    let piston = &callbacks.pistons[0];
    let near = electrical.contacts[piston.near.index()].position;
    let route = &callbacks.routes[piston.routes.head_restored.index()];
    assert!(
        !route.deliveries.iter().any(|delivery| {
            electrical.sites[delivery.target.index()].position == near
                && matches!(delivery.kind, DeliveryKind::Update { direction: None })
        }),
        "native finish skips the restored head's self-recheck"
    );
}

#[test]
fn redstone_payload_with_unmodeled_entity_is_rejected() {
    let mut world = empty();
    world.set_block(BASE, piston_block(BlockFace::East, true, false));
    let payload = BASE.offset(BlockFace::East);
    world.set_block(payload, Block::RedstoneBlock);
    world.set_block_entity(payload, BlockEntity::Comparator { output_strength: 7 });
    assert!(
        matches!(prepare_geometry_for(&world),
        Err(GeometryError::Rejected(GeometryIssue::UnsupportedMaterial { position, .. })) if position == payload),
        "a raw/imported stray entity would be transported natively but has no graph-owned representation"
    );
    assert!(world.get_block_entity(payload).is_some());
}

#[test]
fn inert_supported_sign_does_not_require_an_update_receiver() {
    let mut world = empty();
    world.set_block(
        BASE,
        Block::Lever {
            lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
        },
    );
    world.set_block(BASE.offset(BlockFace::Bottom), Block::Stone {});
    let sign = BASE.offset(BlockFace::East);
    world.set_block(sign, Block::from_name("oak_sign").unwrap());
    world.set_block(sign.offset(BlockFace::Bottom), Block::Stone {});
    assert!(!crate::redstone::has_neighbor_update(world.get_block(sign)));
    let geometry = prepare_geometry_for(&world).unwrap();
    let mut electrical = electrical::prepare_electrical(
        &world,
        geometry.report.bounds,
        &geometry.draft,
        &Default::default(),
    )
    .unwrap();
    let result = callbacks::prepare_callbacks(
        &world,
        &geometry.draft,
        &mut electrical,
        &Default::default(),
    );
    assert!(
        result.is_ok(),
        "ordinary callbacks to a fixed supported sign are inert: {result:?}"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixturePrepStage {
    Geometry,
    Electrical,
    Callbacks,
    GraphValidation,
    Admitted,
}

fn fixture_callback_projection(name: &str) -> (FixturePrepStage, Option<String>) {
    let world = geometry_io_fixture(name);
    let before_ticks = format!("{:?}", world.scheduler().iter_entries().collect::<Vec<_>>());
    let result = (|| {
        let geometry = prepare_geometry_for(&world)
            .map_err(|error| (FixturePrepStage::Geometry, error.to_string()))?;
        let mut electrical = electrical::prepare_electrical(
            &world,
            geometry.report.bounds,
            &geometry.draft,
            &Default::default(),
        )
        .map_err(|error| (FixturePrepStage::Electrical, error.to_string()))?;
        let callbacks = callbacks::prepare_callbacks(
            &world,
            &geometry.draft,
            &mut electrical,
            &Default::default(),
        )
        .map_err(|error| (FixturePrepStage::Callbacks, error.to_string()))?;
        // Electrical query plans are not yet part of CircuitDraft; this checks callback/state bindings.
        let draft = CircuitDraft {
            nodes: electrical.nodes,
            links: callbacks.links,
            power_links: vec![],
            comparator_inputs: vec![],
            profiles: electrical.profiles,
            guards: electrical.guards,
            contacts: electrical.contacts,
            routes: callbacks.routes,
            wires: electrical.wires,
            sites: electrical.sites,
            pistons: callbacks.pistons,
            resources: geometry
                .draft
                .resources
                .iter()
                .map(|resource| resource.spec.clone())
                .collect(),
            initial_pistons: geometry.draft.initial_pistons,
            initial_resources: geometry.draft.initial_resources,
        };
        println!(
            "{name}: nodes={}, routes={}, resources={}",
            draft.nodes.len(),
            draft.routes.len(),
            draft.resources.len()
        );
        draft
            .lower()
            .map_err(|error| (FixturePrepStage::GraphValidation, error.to_string()))?;
        Ok::<_, (FixturePrepStage, String)>(())
    })();
    assert_eq!(
        before_ticks,
        format!("{:?}", world.scheduler().iter_entries().collect::<Vec<_>>())
    );
    match result {
        Ok(()) => (FixturePrepStage::Admitted, None),
        Err((stage, error)) => (stage, Some(error)),
    }
}

fn validate_fixture_callback_projection(name: &str) {
    let (stage, error) = fixture_callback_projection(name);
    assert_eq!(
        stage,
        FixturePrepStage::Admitted,
        "{name}: {stage:?}: {}",
        error.unwrap_or_default()
    );
}

#[test]
fn shared_or_completes_callback_projection_and_preserves_entry() {
    validate_fixture_callback_projection("OR_1.schem");
}

#[test]
fn adder_one_bit_completes_callback_projection_and_preserves_entry() {
    validate_fixture_callback_projection("ADDER_1BIT.schem");
}

#[test]
fn bud_completes_callback_projection_and_preserves_entry() {
    validate_fixture_callback_projection("BUD_PistonUpdate.schem");
}

#[test]
fn remaining_frozen_targets_report_their_first_parser_stage() {
    let fixtures = [
        ("ADDER_11BITS.schem", FixturePrepStage::Admitted),
        ("AND_1.schem", FixturePrepStage::Admitted),
        ("AND_2.schem", FixturePrepStage::Admitted),
        ("AND_3.schem", FixturePrepStage::Geometry),
        (
            "BUD_InstantMemoryCellObserverUpdate.schem",
            FixturePrepStage::Admitted,
        ),
        ("BUD_InstantPistonUpdate.schem", FixturePrepStage::Admitted),
        ("BUD_NonInstantInputs.schem", FixturePrepStage::Admitted),
        ("COUNTER_BASIC.schem", FixturePrepStage::Admitted),
        ("INSTANT_BLOCKED.schem", FixturePrepStage::Admitted),
        ("INSTANT_CHAIN.schem", FixturePrepStage::Admitted),
        ("INSTANT_DOWN.schem", FixturePrepStage::Admitted),
        ("INSTANT_DOWN_TORCH_RESET.schem", FixturePrepStage::Admitted),
        ("INSTANT_OBSERVER.schem", FixturePrepStage::Admitted),
        ("INSTANT_RESET_REDSTONE.schem", FixturePrepStage::Admitted),
        ("INSTANT_RESET_REDSTONE_2.schem", FixturePrepStage::Admitted),
        ("INSTANT_TORCH.schem", FixturePrepStage::Admitted),
        ("NOT_1.schem", FixturePrepStage::Geometry),
        ("OR_Interpreter_illigal.schem", FixturePrepStage::Admitted),
        ("XOR_Simple.schem", FixturePrepStage::Geometry),
    ];
    for (name, expected_stage) in fixtures {
        let (stage, detail) = fixture_callback_projection(name);
        let detail = detail.map_or_else(String::new, |detail| format!(" ({detail})"));
        println!("target stage: {name}: {stage:?}{detail}");
        assert_eq!(stage, expected_stage, "{name}: {detail}");
    }
}
