use super::*;
use mchprs_blocks::BlockFacing;
use mchprs_blocks::blocks::{
    Lever, LeverFace, RedstoneComparator, RedstoneObserver, RedstonePiston, RedstoneRepeater,
};

fn snapshot(bounds: (BlockPos, BlockPos), blocks: &[(BlockPos, Block)]) -> Snapshot {
    let halo = BlockPos::new(14, 14, 14);
    let outer = ((bounds.0 - halo).max(BlockPos::zero()), bounds.1 + halo);
    let size = outer.1 - outer.0 + BlockPos::new(1, 1, 1);
    let mut data = Data {
        _reservation: Reservation::new((size.x * size.y * size.z) as usize * 4).unwrap(),
        bounds: outer,
        route_bounds: bounds,
        world_bounds: outer,
        size,
        blocks: vec![0; (size.x * size.y * size.z) as usize],
        tails: Vec::new(),
        tail_index: FxHashMap::default(),
    };
    for x in bounds.0.x..=bounds.1.x {
        for z in bounds.0.z..=bounds.1.z {
            let p = BlockPos::new(x, bounds.0.y - 1, z);
            let i = data.index(p).unwrap();
            data.blocks[i] = Block::Glass {}.get_id();
        }
    }
    for &(p, block) in blocks {
        let i = data.index(p).unwrap();
        data.blocks[i] = geometry(block);
    }
    Snapshot {
        data: Arc::new(data),
        versions: Arc::new(Vec::new()),
    }
}

fn line() -> Vec<BlockPos> {
    (20..=23).map(|x| BlockPos::new(x, 20, 20)).collect()
}

fn candidate(path: &[BlockPos], blocks: &[(BlockPos, Block)]) -> Result<Plan, String> {
    let first = path.iter().copied().reduce(BlockPos::min).unwrap();
    let last = path.iter().copied().reduce(BlockPos::max).unwrap();
    let snapshot = snapshot((first, last), blocks);
    let cancel = AtomicBool::new(false);
    let budget = Budget::new(&snapshot, path[0], &cancel).unwrap_or_else(|_| panic!("budget"));
    match make_plan(&snapshot, path, &budget) {
        Ok(result) => result,
        Err(_) => panic!("Tiny safety check exceeded its budget"),
    }
}

#[test]
fn flat_wire_has_no_repeaters_or_range_limit() {
    let path: Vec<_> = (20..=59).map(|x| BlockPos::new(x, 20, 20)).collect();
    let plan = candidate(&path, &[]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(plan.path, path);
    assert_eq!(plan.placements.len(), 40);
    assert!(
        plan.placements
            .iter()
            .all(|(_, b)| matches!(b, Block::RedstoneWire { .. }))
    );
}

#[test]
fn transparent_stair_is_rejected_but_conducting_stair_connects_both_ways() {
    let path = [BlockPos::new(20, 20, 20), BlockPos::new(21, 21, 20)];
    let step = BlockPos::new(21, 20, 20);
    assert!(candidate(&path, &[(step, Block::Glass {})]).is_err());
    assert!(candidate(&path, &[(step, Block::Stone {})]).is_ok());
}

#[test]
fn new_staircase_supports_are_opaque_and_work_in_both_directions() {
    let path: Vec<_> = (0..=3).map(|d| BlockPos::new(20 + d, 20 + d, 20)).collect();
    for path in [path.clone(), path.iter().rev().copied().collect()] {
        let plan = candidate(&path, &[]).unwrap_or_else(|e| panic!("{e}"));
        let supports: Vec<_> = plan
            .placements
            .iter()
            .filter(|(_, block)| !matches!(block, Block::RedstoneWire { .. }))
            .collect();
        assert_eq!(supports.len(), 3);
        assert!(supports.iter().all(|(_, block)| matches!(
            block,
            Block::Wool {
                color: mchprs_blocks::BlockColorVariant::White
            }
        )));
        assert!(
            plan.placements
                .iter()
                .take(3)
                .all(|(_, block)| !matches!(block, Block::RedstoneWire { .. }))
        );
    }
}

#[test]
fn vertical_targets_route_to_a_safe_staircase_within_the_budget() {
    let start = BlockPos::new(24, 24, 24);
    for delta in [
        BlockPos::new(4, 4, 0),
        BlockPos::new(4, -4, 0),
        BlockPos::new(0, 3, 0),
    ] {
        let end = start + delta;
        let padding = BlockPos::new(4, 2, 4);
        let snapshot = snapshot((start.min(end) - padding, start.max(end) + padding), &[]);
        let result = search(snapshot, start, end, true, &AtomicBool::new(false));
        let SearchResult::Found(plan) = result else {
            panic!("No safe staircase found for {delta}")
        };
        assert_eq!(plan.path.first(), Some(&start));
        assert_eq!(plan.path.last(), Some(&end));
        assert!(
            plan.placements
                .iter()
                .any(|(_, block)| matches!(block, Block::Wool { .. }))
        );
    }
}

#[test]
fn constructed_staircase_transmits_native_power_in_both_directions() {
    let mut world = empty_world();
    let start = BlockPos::new(32, 20, 32);
    let end = BlockPos::new(35, 23, 32);
    let snapshot = capture_world(&world, start, end);
    let SearchResult::Found(plan) = search(snapshot, start, end, true, &AtomicBool::new(false))
    else {
        panic!("Could not route a native staircase")
    };
    for &(pos, block) in &plan.placements {
        let block = if matches!(block, Block::RedstoneWire { .. }) {
            Block::RedstoneWire {
                wire: wire::get_state_for_placement(&world, pos),
            }
        } else {
            block
        };
        interaction::place_in_world(block, &mut world, pos, &None);
    }
    let source = start.offset(BlockFace::West);
    interaction::place_in_world(Block::RedstoneBlock {}, &mut world, source, &None);
    let Block::RedstoneWire { wire: output } = world.get_block(end) else {
        panic!("Missing upper endpoint")
    };
    assert_eq!(output.power, 12);
    interaction::destroy(Block::RedstoneBlock {}, &mut world, source);
    interaction::place_in_world(
        Block::RedstoneBlock {},
        &mut world,
        end.offset(BlockFace::East),
        &None,
    );
    let Block::RedstoneWire { wire: output } = world.get_block(start) else {
        panic!("Missing lower endpoint")
    };
    assert_eq!(output.power, 12);
}

#[test]
fn new_opaque_stair_supports_cannot_conduct_a_dormant_source() {
    let path = [BlockPos::new(20, 20, 20), BlockPos::new(21, 21, 20)];
    let source = BlockPos::new(21, 20, 21);
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            facing: BlockDirection::South,
            powered: false,
            ..Default::default()
        },
    };
    let error = candidate(
        &path,
        &[
            (source, repeater),
            (source.offset(BlockFace::Bottom), Block::Glass {}),
        ],
    )
    .err()
    .unwrap();
    assert!(error.contains("conduct an unselected"), "{error}");
}

#[test]
fn a_new_stair_conductor_cannot_add_a_comparator_or_qc_input() {
    let path = [BlockPos::new(20, 20, 20), BlockPos::new(21, 21, 20)];
    let comparator = Block::RedstoneComparator {
        comparator: RedstoneComparator {
            facing: BlockDirection::North,
            ..Default::default()
        },
    };
    let error = candidate(
        &path,
        &[
            (BlockPos::new(21, 20, 21), comparator),
            (BlockPos::new(21, 19, 21), Block::Glass {}),
        ],
    )
    .err()
    .unwrap();
    assert!(error.contains("unintended input"), "{error}");
    let piston = Block::Piston {
        piston: RedstonePiston {
            facing: BlockFacing::East,
            sticky: false,
            extended: false,
        },
    };
    assert!(candidate(&path, &[(BlockPos::new(21, 19, 21), piston)]).is_err());
}

#[test]
fn missing_supports_are_nonconducting_and_placed_before_dust() {
    let p = BlockPos::new(21, 19, 20);
    let plan = candidate(&line(), &[(p, Block::Air {})]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(plan.placements[0], (p, Block::Glass {}));
}

#[test]
fn new_supports_sample_passive_start_material_and_preserve_color() {
    let path = line();
    for material in [
        Block::Stone {},
        Block::Sandstone {},
        Block::Wool {
            color: mchprs_blocks::BlockColorVariant::Blue,
        },
    ] {
        let blocks: Vec<_> = path
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    p.offset(BlockFace::Bottom),
                    if i == 0 { material } else { Block::Air {} },
                )
            })
            .collect();
        let plan = candidate(&path, &blocks).unwrap_or_else(|e| panic!("{e}"));
        for p in &path[1..] {
            assert!(
                plan.placements
                    .contains(&(p.offset(BlockFace::Bottom), material))
            );
        }
        let raised = [path[0], path[1].offset(BlockFace::Top)];
        let plan = candidate(&raised, &[(path[0].offset(BlockFace::Bottom), material)])
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(plan.placements.contains(&(path[1], material)));
    }
}

#[test]
fn support_sampling_never_copies_components_or_nbt_and_opaque_flats_keep_power_guards() {
    for block in [
        Block::RedstoneBlock {},
        Block::RedstoneLamp { lit: false },
        Block::Barrel {
            facing: BlockFacing::Up,
            open: false,
        },
        Block::Target {},
        Block::Piston {
            piston: RedstonePiston::default(),
        },
    ] {
        assert_eq!(passive_support(block), Block::Glass {});
    }
    let path = line();
    let source = BlockPos::new(21, 19, 21);
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            facing: BlockDirection::South,
            powered: false,
            ..Default::default()
        },
    };
    let error = candidate(
        &path,
        &[
            (path[0].offset(BlockFace::Bottom), Block::Stone {}),
            (path[1].offset(BlockFace::Bottom), Block::Air {}),
            (source, repeater),
            (source.offset(BlockFace::Bottom), Block::Glass {}),
        ],
    )
    .err()
    .unwrap();
    assert!(error.contains("conduct an unselected"), "{error}");
}

#[test]
fn dormant_sources_and_strong_power_through_a_conductor_are_rejected() {
    let conductor = BlockPos::new(21, 20, 21);
    let source = conductor.offset(BlockFace::Top);
    let lever = Block::Lever {
        lever: Lever {
            face: LeverFace::Floor,
            facing: BlockDirection::North,
            powered: false,
        },
    };
    let error = candidate(&line(), &[(conductor, Block::Stone {}), (source, lever)])
        .err()
        .unwrap();
    assert!(error.contains("inject"), "{error}");
}

#[test]
fn merging_unselected_dust_is_rejected_but_parallel_lanes_at_spacing_two_work() {
    let extra = BlockPos::new(21, 20, 21);
    let error = candidate(&line(), &[(extra, plain_wire())]).err().unwrap();
    assert!(error.contains("merge"), "{error}");
    let blocks: Vec<_> = line()
        .iter()
        .flat_map(|p| {
            let wire = *p + BlockPos::new(0, 0, 2);
            [
                (wire, plain_wire()),
                (wire.offset(BlockFace::Bottom), Block::Glass {}),
            ]
        })
        .collect();
    assert!(candidate(&line(), &blocks).is_ok());
}

#[test]
fn only_selected_endpoint_dust_may_join_the_route() {
    let path = line();
    assert!(
        candidate(
            &path,
            &[
                (path[0], plain_wire()),
                (*path.last().unwrap(), plain_wire())
            ]
        )
        .is_ok()
    );
    assert!(candidate(&path, &[(path[1], plain_wire())]).is_err());
}

#[test]
fn observer_watch_and_independent_piston_notifications_are_rejected() {
    let observer = Block::Observer {
        observer: RedstoneObserver {
            facing: BlockFacing::North,
            powered: false,
        },
    };
    let error = candidate(&line(), &[(BlockPos::new(21, 20, 21), observer)])
        .err()
        .unwrap();
    assert!(error.contains("observer"), "{error}");
    let piston = Block::Piston {
        piston: RedstonePiston {
            facing: BlockFacing::East,
            sticky: false,
            extended: false,
        },
    };
    let error = candidate(&line(), &[(BlockPos::new(21, 18, 20), piston)])
        .err()
        .unwrap();
    assert!(
        error.contains("piston") || error.contains("Piston"),
        "{error}"
    );
}

#[test]
fn qc_power_through_an_above_base_conductor_is_rejected_without_a_notification() {
    let conductor = BlockPos::new(21, 20, 21);
    let piston = Block::Piston {
        piston: RedstonePiston {
            facing: BlockFacing::East,
            sticky: false,
            extended: false,
        },
    };
    let error = candidate(
        &line(),
        &[
            (conductor, Block::Stone {}),
            (BlockPos::new(21, 19, 22), piston),
        ],
    )
    .err()
    .unwrap();
    assert!(error.contains("unintended input"), "{error}");
}

#[test]
fn comparator_side_and_hopper_inputs_are_rejected_but_repeater_side_is_safe() {
    let at = BlockPos::new(21, 20, 21);
    let comparator = Block::RedstoneComparator {
        comparator: RedstoneComparator {
            facing: BlockDirection::East,
            ..Default::default()
        },
    };
    assert!(candidate(&line(), &[(at, comparator)]).is_err());
    let hopper = Block::Hopper {
        facing: mchprs_blocks::blocks::HopperFacing::Down,
        enabled: true,
    };
    assert!(candidate(&line(), &[(at, hopper)]).is_err());
    let repeater = Block::RedstoneRepeater {
        repeater: RedstoneRepeater {
            facing: BlockDirection::East,
            ..Default::default()
        },
    };
    assert!(candidate(&line(), &[(at, repeater)]).is_ok());
}

#[test]
fn dust_cannot_mute_an_existing_note_block() {
    let block = Block::NoteBlock {
        instrument: mchprs_blocks::blocks::Instrument::Harp,
        note: 0,
        powered: false,
    };
    let error = candidate(&line(), &[(BlockPos::new(21, 19, 20), block)])
        .err()
        .unwrap();
    assert!(error.contains("mute"), "{error}");
}

#[test]
fn a_new_glass_support_cannot_mute_an_existing_note_block() {
    let block = Block::NoteBlock {
        instrument: mchprs_blocks::blocks::Instrument::Harp,
        note: 0,
        powered: false,
    };
    let error = candidate(
        &line(),
        &[
            (BlockPos::new(21, 19, 20), Block::Air {}),
            (BlockPos::new(21, 18, 20), block),
        ],
    )
    .err()
    .unwrap();
    assert!(error.contains("mute"), "{error}");
}

#[test]
fn registry_bulb_and_pressure_plate_outputs_do_not_change_geometry() {
    for name in [
        "copper_bulb",
        "waxed_exposed_copper_bulb",
        "oak_pressure_plate",
        "cherry_pressure_plate",
    ] {
        let original = Block::from_name(name).unwrap();
        let powered = if original.is_copper_bulb() {
            original.with_copper_bulb_state(true, true).unwrap()
        } else {
            original.with_pressure_plate_power(true).unwrap()
        };
        let normalized = Block::from_id(geometry(powered));
        assert_eq!(geometry(original), geometry(powered), "{name}");
        assert_eq!(normalized.get_name(), powered.get_name());
        for (property, value) in powered.properties() {
            if property != "powered" && property != "lit" {
                assert_eq!(
                    normalized.properties()[property],
                    value,
                    "{name}: {property}"
                );
            }
        }
    }
    let trapdoor = Block::from_name("oak_trapdoor").unwrap();
    let mut changed = trapdoor;
    changed.set_properties(std::collections::HashMap::from([("powered", "true")]));
    assert_ne!(
        geometry(trapdoor),
        geometry(changed),
        "unsupported powered states must remain exact"
    );
}

#[test]
fn power_only_updates_do_not_change_the_routing_revision() {
    let mut chunk = Chunk::empty(0, 0);
    let mut dust = RedstoneWire::default();
    chunk.set_block(1, 1, 1, Block::RedstoneWire { wire: dust }.get_id());
    let routing = chunk.routing_snapshot_version();
    let raw = chunk.snapshot_version();
    for power in 1..=15 {
        dust.power = power;
        chunk.set_block(1, 1, 1, Block::RedstoneWire { wire: dust }.get_id());
        assert_eq!(chunk.routing_snapshot_version(), routing);
    }
    assert_ne!(chunk.snapshot_version(), raw);
    dust.east = mchprs_blocks::blocks::RedstoneWireSide::Side;
    chunk.set_block(1, 1, 1, Block::RedstoneWire { wire: dust }.get_id());
    assert_ne!(chunk.routing_snapshot_version(), routing);
}

fn empty_world() -> PlotWorld {
    let chunks = (0..crate::plot::PLOT_WIDTH)
        .flat_map(|x| (0..crate::plot::PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

fn generated_world(layers: i32) -> PlotWorld {
    let chunks = (0..crate::plot::PLOT_WIDTH)
        .flat_map(|x| {
            (0..crate::plot::PLOT_WIDTH)
                .map(move |z| crate::plot::Plot::generate_chunk(layers, x, z))
        })
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

#[test]
fn straight_wire_on_the_real_generated_floor_is_found() {
    let world = generated_world(8);
    let start = BlockPos::new(32, 8, 32);
    for distance in [1, 8, 32, 64] {
        let end = start + BlockPos::new(distance, 0, 0);
        let snapshot = capture_world(&world, start, end);
        let result = search(snapshot, start, end, true, &AtomicBool::new(false));
        let SearchResult::Found(plan) = result else {
            panic!("Generated-floor line of {distance} blocks was not found")
        };
        assert_eq!(plan.path.len(), distance as usize + 1);
        assert!(
            plan.placements
                .iter()
                .all(|(_, block)| matches!(block, Block::RedstoneWire { .. }))
        );
    }
}

#[test]
fn mixed_xyz_clear_routes_make_progress_before_enumerating_height_prefixes() {
    let mut world = generated_world(8);
    let start = BlockPos::new(32, 21, 35);
    let end = start + BlockPos::new(12, 3, 12);
    for p in [start, end] {
        world.set_block(p.offset(BlockFace::Bottom), Block::Stone {});
    }
    let snapshot = capture_world(&world, start, end);
    for prefer_x in [true, false] {
        let plan = match search(
            snapshot.clone(),
            start,
            end,
            prefer_x,
            &AtomicBool::new(false),
        ) {
            SearchResult::Found(plan) => plan,
            SearchResult::BudgetExceeded => panic!("Clear XYZ route exhausted its budget"),
            SearchResult::Invalid(reason) => panic!("Clear XYZ route was refused: {reason}"),
            _ => panic!("Clear XYZ route was not found"),
        };
        assert_eq!(plan.path.len(), 25);
        assert_eq!(plan.path.first(), Some(&start));
        assert_eq!(plan.path.last(), Some(&end));
    }
}

#[test]
fn dense_machine_route_avoids_a_strongly_powered_support_near_the_start() {
    let mut world = generated_world(8);
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(44, 20, 20);
    for p in [start, end] {
        world.set_block(p.offset(BlockFace::Bottom), Block::Stone {});
    }
    let hidden_wire = BlockPos::new(22, 19, 21);
    world.set_block(hidden_wire.offset(BlockFace::Bottom), Block::Glass {});
    world.set_block(hidden_wire.offset(BlockFace::Top), Block::Stone {});
    world.set_block(
        hidden_wire,
        Block::RedstoneWire {
            wire: wire::get_state_for_placement(&world, hidden_wire),
        },
    );
    let snapshot = capture_world(&world, start, end);
    let plan = match search(snapshot, start, end, true, &AtomicBool::new(false)) {
        SearchResult::Found(plan) => plan,
        SearchResult::BudgetExceeded => panic!("Valid route near hidden dust exhausted its budget"),
        SearchResult::Invalid(reason) => {
            panic!("Valid route near hidden dust was refused: {reason}")
        }
        _ => panic!("Valid route near hidden dust was not found"),
    };
    assert!(
        !plan
            .placements
            .iter()
            .any(|&(p, block)| { p == BlockPos::new(22, 19, 20) && block.is_solid() })
    );
}

#[test]
fn dense_machine_route_finds_a_flat_detour_around_a_late_consumer() {
    let mut world = generated_world(8);
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(44, 20, 20);
    for x in 16..=48 {
        for z in 16..=24 {
            for y in [19, 21] {
                world.set_block(BlockPos::new(x, y, z), Block::Stone {});
            }
        }
    }
    let consumer = BlockPos::new(42, 20, 20);
    world.set_block(consumer, Block::RedstoneLamp { lit: false });
    let snapshot = capture_world(&world, start, end);
    let plan = match search(snapshot, start, end, true, &AtomicBool::new(false)) {
        SearchResult::Found(plan) => plan,
        SearchResult::BudgetExceeded => panic!("Obvious flat consumer detour exhausted its budget"),
        SearchResult::Invalid(reason) => panic!("Flat consumer detour was refused: {reason}"),
        _ => panic!("Flat consumer detour was not found"),
    };
    assert!(plan.path.iter().all(|p| p.y == 20));
    assert!(plan.path.iter().any(|p| p.z == 18 || p.z == 22));
}

#[test]
fn unavoidable_endpoint_observer_is_reported_without_exhausting_the_search() {
    let mut world = generated_world(8);
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(44, 20, 20);
    for p in [start, end] {
        world.set_block(p.offset(BlockFace::Bottom), Block::Glass {});
    }
    world.set_block(
        end.offset(BlockFace::East),
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::West,
                powered: false,
            },
        },
    );
    let snapshot = capture_world(&world, start, end);
    let result = search(snapshot, start, end, true, &AtomicBool::new(false));
    assert!(matches!(result, SearchResult::Invalid(ref reason) if reason.contains("observer")));
}

#[test]
fn selected_power_sources_feed_routes_away_and_along_the_same_emitter() {
    let start = BlockPos::new(20, 20, 20);
    for end in [BlockPos::new(24, 20, 20), BlockPos::new(21, 20, 23)] {
        let mut world = generated_world(8);
        world.set_block(start.offset(BlockFace::Bottom), Block::Glass {});
        world.set_block(BlockPos::new(20, 20, 21), Block::RedstoneBlock {});
        let result = search(
            capture_world(&world, start, end),
            start,
            end,
            true,
            &AtomicBool::new(false),
        );
        assert!(
            matches!(result, SearchResult::Found(_)),
            "Selected source was refused for {end}"
        );
    }
}

#[test]
fn selected_support_powered_by_dust_does_not_allow_other_sources() {
    let mut world = generated_world(8);
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(21, 20, 23);
    let source = BlockPos::new(20, 19, 21);
    world.set_block(start.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(source.offset(BlockFace::Bottom), Block::Glass {});
    world.set_block(source.offset(BlockFace::Top), Block::Stone {});
    world.set_block(
        source,
        Block::RedstoneWire {
            wire: wire::get_state_for_placement(&world, source),
        },
    );
    let result = search(
        capture_world(&world, start, end),
        start,
        end,
        true,
        &AtomicBool::new(false),
    );
    assert!(
        matches!(result, SearchResult::Found(_)),
        "Selected powered support was refused"
    );
    world.set_block(end.offset(BlockFace::East), Block::RedstoneBlock {});
    let result = search(
        capture_world(&world, start, end),
        start,
        end,
        true,
        &AtomicBool::new(false),
    );
    assert!(matches!(result, SearchResult::Invalid(ref reason) if reason.contains("inject")));
}

#[test]
fn selected_incoming_step_dust_does_not_expand_to_the_whole_net() {
    let mut world = generated_world(8);
    let start = BlockPos::new(20, 20, 20);
    let incoming = BlockPos::new(20, 21, 21);
    let unrelated = incoming.offset(BlockFace::East);
    world.set_block(start.offset(BlockFace::Bottom), Block::Stone {});
    world.set_block(start, plain_wire());
    for pos in [incoming, unrelated] {
        world.set_block(pos.offset(BlockFace::Bottom), Block::Glass {});
        world.set_block(
            pos,
            Block::RedstoneWire {
                wire: wire::get_state_for_placement(&world, pos),
            },
        );
    }
    world.set_block(
        start,
        Block::RedstoneWire {
            wire: wire::get_state_for_placement(&world, start),
        },
    );
    let end = BlockPos::new(20, 20, 16);
    let snapshot = capture_world(&world, start, end);
    let cancel = AtomicBool::new(false);
    let budget = Budget::new(&snapshot, start, &cancel).unwrap_or_else(|_| panic!("budget"));
    assert!(budget.selected_sources.contains(&incoming));
    assert!(!budget.selected_sources.contains(&unrelated));
    assert!(matches!(
        search(snapshot, start, end, true, &cancel),
        SearchResult::Found(_)
    ));
}

#[test]
fn endpoint_near_machinery_can_route_along_an_unchanged_connection_side() {
    let mut world = generated_world(8);
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(24, 20, 20);
    let old_neighbor = start.offset(BlockFace::South);
    for pos in [start, old_neighbor] {
        world.set_block(pos.offset(BlockFace::Bottom), Block::Glass {});
        world.set_block(pos, plain_wire());
    }
    world.set_block(
        start,
        Block::RedstoneWire {
            wire: wire::get_state_for_placement(&world, start),
        },
    );
    world.set_block(
        start + BlockPos::new(0, -2, 0),
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                extended: false,
                sticky: false,
            },
        },
    );
    let snapshot = capture_world(&world, start, end);
    let cancel = AtomicBool::new(false);
    let budget = Budget::new(&snapshot, start, &cancel).unwrap_or_else(|_| panic!("budget"));
    let before = View::new(&snapshot, &budget);
    assert!(failure(&before, start, true).is_some());
    let SearchResult::Found(plan) = search(snapshot, start, end, true, &cancel) else {
        panic!("An unchanged endpoint was blanket-refused near machinery")
    };
    assert_eq!(plan.path[1], start.offset(BlockFace::North));
}

#[test]
#[ignore = "loads the full author PM1 schematic for routing safety/performance checks"]
fn pm1_short_connections_from_selected_sources_have_bounded_precise_results() {
    use crate::plot::worldedit::{load_schematic, paste_clipboard};
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test_data/piston-research/pm1-compilation-1-20261008-fresh/PM1_FIXED_COMPILATION_1.schem"
    ));
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "cf5ef6b5e62defbc02dc3b201b9bb29766feb310f6abfcad2941486312e0bd7c"
    );
    let mut clipboard = load_schematic(std::io::Cursor::new(bytes)).unwrap();
    clipboard.offset_x = 0;
    clipboard.offset_y = 0;
    clipboard.offset_z = 0;
    let origin = BlockPos::new(8, 8, 8);
    let mut world = empty_world();
    paste_clipboard(&mut world, &clipboard, origin, false);
    let cases = [
        (BlockPos::new(118, 34, 30), BlockPos::new(115, 34, 30), None),
        (BlockPos::new(124, 34, 30), BlockPos::new(127, 34, 30), None),
        (
            BlockPos::new(170, 34, 36),
            BlockPos::new(170, 34, 33),
            Some("geometry"),
        ),
        (
            BlockPos::new(190, 34, 40),
            BlockPos::new(193, 34, 40),
            Some("Piston movement"),
        ),
        (
            BlockPos::new(64, 34, 85),
            BlockPos::new(64, 34, 82),
            Some("Moving geometry"),
        ),
        (
            BlockPos::new(100, 34, 103),
            BlockPos::new(100, 34, 106),
            Some("Moving geometry"),
        ),
        (BlockPos::new(165, 64, 71), BlockPos::new(165, 64, 74), None),
        (BlockPos::new(157, 64, 72), BlockPos::new(157, 64, 75), None),
        (
            BlockPos::new(145, 153, 107),
            BlockPos::new(148, 153, 107),
            None,
        ),
        (
            BlockPos::new(149, 176, 107),
            BlockPos::new(152, 176, 107),
            None,
        ),
        (
            BlockPos::new(157, 213, 108),
            BlockPos::new(160, 213, 108),
            None,
        ),
        (
            BlockPos::new(160, 213, 108),
            BlockPos::new(163, 213, 108),
            None,
        ),
        (
            BlockPos::new(167, 52, 12),
            BlockPos::new(170, 52, 12),
            Some("BUD/QC"),
        ),
        (
            BlockPos::new(192, 96, 70),
            BlockPos::new(195, 96, 70),
            Some("unintended input"),
        ),
    ];
    let mut failures = Vec::new();
    for (local_start, local_end, expected) in cases {
        let start = local_start + origin;
        let end = local_end + origin;
        let mut capture = Capture::new(&world, start, end).unwrap();
        let began = Instant::now();
        let snapshot = loop {
            if let Some(snapshot) = capture.step(&world).unwrap() {
                break snapshot;
            }
        };
        let capture_time = began.elapsed();
        let reads = capture.reads;
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(&snapshot, start, &cancel).unwrap_or_else(|_| panic!("budget"));
        let d = end - start;
        let length = d.x.abs() + d.z.abs();
        let step = BlockPos::new(d.x.signum(), 0, d.z.signum());
        let direct: Vec<_> = (0..=length)
            .map(|n| start + BlockPos::new(step.x * n, 0, step.z * n))
            .collect();
        if let Ok(Err(reason)) = make_plan(&snapshot, &direct, &budget) {
            eprintln!(
                "PM1 direct {local_start}: selected={:?}; {reason}",
                budget.selected_sources
            );
        }
        let began = Instant::now();
        let result = search(snapshot, start, end, true, &AtomicBool::new(false));
        let outcome = match &result {
            SearchResult::Found(_) => "Found".to_owned(),
            SearchResult::Invalid(reason) => format!("Invalid: {reason}"),
            SearchResult::BudgetExceeded => "BudgetExceeded".to_owned(),
            SearchResult::NoPath => "NoPath".to_owned(),
            SearchResult::Cancelled => "Cancelled".to_owned(),
        };
        eprintln!(
            "PM1 {local_start}->{local_end}: capture={capture_time:?}, rays={reads}, search={:?}, {outcome}",
            began.elapsed()
        );
        let matches_expected = match expected {
            None => matches!(result, SearchResult::Found(_)),
            Some(expected) => {
                matches!(result, SearchResult::Invalid(ref reason) if reason.contains(expected))
            }
        };
        if !matches_expected {
            failures.push(format!("{local_start}->{local_end}: {outcome}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn consecutive_native_segments_extend_and_turn_along_existing_glass_supports() {
    let mut world = generated_world(8);
    let mut start = BlockPos::new(32, 21, 32);
    world.set_block(start.offset(BlockFace::Bottom), Block::Glass {});
    for end in [
        BlockPos::new(47, 21, 32),
        BlockPos::new(62, 21, 32),
        BlockPos::new(77, 21, 32),
        BlockPos::new(77, 21, 44),
    ] {
        let snapshot = capture_world(&world, start, end);
        let path: Vec<_> = if start.z == end.z {
            (start.x..=end.x)
                .map(|x| BlockPos::new(x, start.y, start.z))
                .collect()
        } else {
            (start.z..=end.z)
                .map(|z| BlockPos::new(start.x, start.y, z))
                .collect()
        };
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(&snapshot, start, &cancel).unwrap_or_else(|_| panic!("budget"));
        if let Ok(Err(reason)) = make_plan(&snapshot, &path, &budget) {
            eprintln!("Direct segment {start} -> {end}: {reason}");
        }
        let plan = match search(snapshot, start, end, true, &cancel) {
            SearchResult::Found(plan) => plan,
            SearchResult::BudgetExceeded => panic!("Segment {start} -> {end}: route limit reached"),
            SearchResult::Invalid(reason) => panic!("Segment {start} -> {end}: {reason}"),
            _ => panic!("Segment {start} -> {end}: no route"),
        };
        assert_eq!(plan.path, path);
        for &(pos, block) in &plan.placements {
            let block = if matches!(block, Block::RedstoneWire { .. }) {
                Block::RedstoneWire {
                    wire: wire::get_state_for_placement(&world, pos),
                }
            } else {
                block
            };
            interaction::place_in_world(block, &mut world, pos, &None);
        }
        start = end;
    }
}

fn long_support_row() -> (PlotWorld, BlockPos, BlockPos) {
    let mut world = generated_world(8);
    let start = BlockPos::new(80, 21, 32);
    let end = BlockPos::new(88, 21, 32);
    for x in 24..=80 {
        world.set_block(BlockPos::new(x, 20, 32), Block::Glass {});
    }
    interaction::place_in_world(
        Block::RedstoneWire {
            wire: wire::get_state_for_placement(&world, start),
        },
        &mut world,
        start,
        &None,
    );
    (world, start, end)
}

#[test]
fn long_mechanical_tails_are_sparse_checked_and_stop_at_captured_air() {
    let (mut world, start, end) = long_support_row();
    let snapshot = capture_world(&world, start, end);
    let stopper = BlockPos::new(23, 20, 32);
    assert!(snapshot.data.index(stopper).is_none());
    assert_eq!(snapshot.data.cell(stopper), Some(geometry(Block::Air {})));
    assert!(snapshot.data.tails.len() < 64);
    assert!(snapshot.data.cells() <= MAX_CELLS);
    assert!(snapshot.bytes() <= MAX_SNAPSHOT_BYTES);
    assert!(matches!(
        search(snapshot.clone(), start, end, true, &AtomicBool::new(false)),
        SearchResult::Found(_)
    ));
    world.set_block(stopper, Block::Stone {});
    assert!(!snapshot.is_current(&world));
    assert!(!snapshot.validate_live(&world));
}

#[test]
fn a_piston_beyond_the_old_halo_still_blocks_an_occupied_payload_ray() {
    let (mut world, start, end) = long_support_row();
    let piston = BlockPos::new(24, 20, 32);
    world.set_block(
        piston,
        Block::Piston {
            piston: RedstonePiston {
                facing: BlockFacing::East,
                sticky: false,
                extended: false,
            },
        },
    );
    let snapshot = capture_world(&world, start, end);
    assert!(snapshot.data.index(piston).is_none());
    assert!(matches!(
        Block::from_id(snapshot.data.cell(piston).unwrap()),
        Block::Piston { .. }
    ));
    let path: Vec<_> = (start.x..=end.x)
        .map(|x| BlockPos::new(x, start.y, start.z))
        .collect();
    let cancel = AtomicBool::new(false);
    let budget = Budget::new(&snapshot, start, &cancel).unwrap_or_else(|_| panic!("budget"));
    let error = make_plan(&snapshot, &path, &budget)
        .unwrap_or_else(|_| panic!("budget"))
        .err()
        .unwrap();
    assert!(error.contains("Piston movement"), "{error}");
}

#[test]
fn incremental_tail_capture_rejects_geometry_changed_after_its_read() {
    let (mut world, start, end) = long_support_row();
    let mut capture = Capture::new(&world, start, end).unwrap();
    let mut changed = false;
    let snapshot = loop {
        if !changed {
            if let Some(data) = &capture.data {
                if let Some(&(p, _)) = data.tails.first() {
                    world.set_block(p, Block::Stone {});
                    changed = true;
                }
            }
        }
        if let Some(snapshot) = capture.step(&world).unwrap() {
            break snapshot;
        }
    };
    assert!(changed, "tail capture must remain cooperative");
    assert!(!snapshot.validate_live(&world));
}

fn capture_world(world: &PlotWorld, start: BlockPos, end: BlockPos) -> Snapshot {
    let mut capture = Capture::new(world, start, end).unwrap();
    loop {
        if let Some(snapshot) = capture.step(world).unwrap() {
            return snapshot;
        }
    }
}

fn finish_check(check: &mut GeometryCheck, world: &PlotWorld) -> bool {
    loop {
        if let Some(result) = check.step(world) {
            return result;
        }
    }
}

#[test]
fn irrelevant_same_chunk_geometry_changes_refresh_the_commit_stamp() {
    let mut world = empty_world();
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(23, 20, 20);
    let snapshot = capture_world(&world, start, end);
    assert!(snapshot.bytes() <= MAX_CELLS * 4);
    // Same captured chunk, but outside the snapshot's vertical context.
    world.set_block(BlockPos::new(20, 100, 20), Block::Stone {});
    assert!(!snapshot.is_current(&world));
    let mut check = GeometryCheck::new(snapshot.clone());
    assert!(finish_check(&mut check, &world));
    let checked = check.checked_snapshot();
    assert!(Arc::ptr_eq(&snapshot.data, &checked.data));
    assert!(checked.is_current(&world));
    world.set_block(BlockPos::new(21, 20, 20), Block::Stone {});
    assert!(!checked.validate_live(&world));
}

#[test]
fn incremental_capture_cannot_certify_a_mixed_time_world() {
    let mut world = empty_world();
    let mut capture =
        Capture::new(&world, BlockPos::new(20, 20, 20), BlockPos::new(23, 20, 20)).unwrap();
    let first = capture.data.as_ref().unwrap().bounds.0;
    assert!(capture.step(&world).unwrap().is_none());
    world.set_block(first, Block::Stone {});
    let snapshot = loop {
        if let Some(snapshot) = capture.step(&world).unwrap() {
            break snapshot;
        }
    };
    assert!(!snapshot.validate_live(&world));
}

#[test]
fn live_power_churn_does_not_starve_a_geometry_check() {
    let mut world = empty_world();
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(23, 20, 20);
    let mut dust = RedstoneWire::default();
    world.set_block(start, Block::RedstoneWire { wire: dust });
    let snapshot = capture_world(&world, start, end);
    let mut check = GeometryCheck::new(snapshot.clone());
    for power in 1..=15 {
        dust.power = power;
        world.set_block(start, Block::RedstoneWire { wire: dust });
        assert!(snapshot.is_current(&world));
        assert_eq!(check.step(&world), Some(true));
    }
}

fn oracle(snapshot: &Snapshot, path: &mut Vec<BlockPos>, end: BlockPos) -> bool {
    let pos = *path.last().unwrap();
    if pos == end {
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(snapshot, path[0], &cancel).unwrap_or_else(|_| panic!("budget"));
        return matches!(make_plan(snapshot, path, &budget), Ok(Ok(_)));
    }
    for side in [
        BlockFace::East,
        BlockFace::West,
        BlockFace::North,
        BlockFace::South,
    ] {
        let next = pos.offset(side);
        if !contains(snapshot.data.route_bounds, next) || path.contains(&next) {
            continue;
        }
        if snapshot.data.blocks[snapshot.data.index(next).unwrap()] != 0 {
            continue;
        }
        path.push(next);
        if oracle(snapshot, path, end) {
            return true;
        }
        path.pop();
    }
    false
}

#[test]
fn astar_matches_exhaustive_tiny_world_oracle_for_every_obstacle_layout() {
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(23, 20, 21);
    let cells: Vec<_> = (20..=23)
        .flat_map(|x| (20..=21).map(move |z| BlockPos::new(x, 20, z)))
        .filter(|p| *p != start && *p != end)
        .collect();
    for mask in 0u32..1 << cells.len() {
        let obstacles: Vec<_> = cells
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, &p)| (p, Block::Stone {}))
            .collect();
        let snapshot = snapshot((start, end), &obstacles);
        let expected = oracle(&snapshot, &mut vec![start], end);
        for prefer_x in [false, true] {
            let result = search(
                snapshot.clone(),
                start,
                end,
                prefer_x,
                &AtomicBool::new(false),
            );
            assert!(
                !matches!(result, SearchResult::BudgetExceeded),
                "tiny layout {mask} exceeded budget"
            );
            assert_eq!(
                matches!(result, SearchResult::Found(_)),
                expected,
                "layout {mask}, prefer_x {prefer_x}"
            );
        }
    }
}

#[test]
fn astar_routes_around_unselected_wire_contacts_and_consumer_inputs() {
    let start = BlockPos::new(20, 20, 22);
    let end = BlockPos::new(25, 20, 22);
    let obstacle = BlockPos::new(22, 20, 22);
    for block in [plain_wire(), Block::RedstoneLamp { lit: false }] {
        let snapshot = snapshot(
            (BlockPos::new(20, 20, 20), BlockPos::new(25, 20, 24)),
            &[(obstacle, block)],
        );
        let SearchResult::Found(plan) = search(snapshot, start, end, true, &AtomicBool::new(false))
        else {
            panic!("No safe detour around {block:?}")
        };
        assert!(plan.path.iter().all(|&p| {
            let d = p - obstacle;
            d.x.abs() + d.z.abs() >= 2
        }));
        assert!(plan.path.iter().any(|p| p.z == 20 || p.z == 24));
    }
}

#[test]
fn oversized_direct_candidate_does_not_hide_a_supported_detour() {
    let start = BlockPos::new(32, 7, 32);
    let end = BlockPos::new(289, 7, 32);
    let gaps: Vec<_> = (start.x + 1..end.x)
        .map(|x| (BlockPos::new(x, start.y - 1, start.z), Block::Air {}))
        .collect();
    let snapshot = snapshot(
        (start - BlockPos::new(0, 0, 1), end + BlockPos::new(0, 0, 1)),
        &gaps,
    );
    let cancel = AtomicBool::new(false);
    let budget = Budget::new(&snapshot, start, &cancel).unwrap_or_else(|_| panic!("budget"));
    let direct: Vec<_> = (start.x..=end.x)
        .map(|x| BlockPos::new(x, start.y, start.z))
        .collect();
    let direct = proposed(&snapshot, &direct, &budget).unwrap();
    assert!(direct.supports.len() + direct.dust.len() > MAX_PLACEMENTS);

    let mut detour = vec![start];
    detour.extend((start.x..=end.x).map(|x| BlockPos::new(x, start.y, start.z + 1)));
    detour.push(end);
    let plan = make_plan(&snapshot, &detour, &budget)
        .unwrap_or_else(|_| panic!("budget"))
        .unwrap();
    assert_eq!(plan.placements.len(), 260);

    assert!(matches!(
        search(snapshot, start, end, true, &cancel),
        SearchResult::Found(_)
    ));
}

#[test]
fn cancellation_and_placement_budget_have_distinct_results() {
    let start = BlockPos::new(20, 20, 20);
    let end = BlockPos::new(23, 20, 20);
    assert!(matches!(
        search(
            snapshot((start, end), &[]),
            start,
            end,
            true,
            &AtomicBool::new(true)
        ),
        SearchResult::Cancelled
    ));
    let far = BlockPos::new(22 + MAX_PLACEMENTS as i32, 20, 20);
    assert!(matches!(
        search(
            snapshot((start, end), &[]),
            start,
            far,
            true,
            &AtomicBool::new(false)
        ),
        SearchResult::BudgetExceeded
    ));
}
