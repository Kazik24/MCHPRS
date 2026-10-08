use super::*;
use mchprs_blocks::blocks::{
    Lever, LeverFace, RedstoneComparator, RedstoneObserver, RedstonePiston, RedstoneRepeater,
};
use mchprs_blocks::BlockFacing;

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
    let budget = Budget {
        cancel: &cancel,
        started: Instant::now(),
        visits: Cell::new(0),
        missing: Cell::new(false),
    };
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
    assert!(plan
        .placements
        .iter()
        .all(|(_, b)| matches!(b, Block::RedstoneWire { .. })));
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
        assert!(plan
            .placements
            .iter()
            .take(3)
            .all(|(_, block)| !matches!(block, Block::RedstoneWire { .. })));
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
        assert!(plan
            .placements
            .iter()
            .any(|(_, block)| matches!(block, Block::Wool { .. })));
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
    assert!(candidate(
        &path,
        &[
            (path[0], plain_wire()),
            (*path.last().unwrap(), plain_wire())
        ]
    )
    .is_ok());
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
        assert!(plan
            .placements
            .iter()
            .all(|(_, block)| matches!(block, Block::RedstoneWire { .. })));
    }
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
        let budget = Budget {
            cancel: &cancel,
            started: Instant::now(),
            visits: Cell::new(0),
            missing: Cell::new(false),
        };
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
