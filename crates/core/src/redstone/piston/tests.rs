//! Timing, lifecycle and completion regressions; movement rules are deferred.
use crate::plot::worldedit::{load_schematic, paste_clipboard};
use crate::plot::{PlotWorld, PLOT_WIDTH};
use crate::world::storage::Chunk;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, MovingPistonEntity};
use mchprs_blocks::blocks::{Block, RedstoneObserver, RedstonePiston, RedstonePistonHead};
use mchprs_blocks::{BlockFace, BlockFacing, BlockPos};

fn empty_world() -> PlotWorld {
    let chunks = (0..PLOT_WIDTH)
        .flat_map(|x| (0..PLOT_WIDTH).map(move |z| Chunk::empty(x, z)))
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}

#[test]
fn dust_shape_change_does_not_start_piston_observer_early() {
    use sha2::{Digest, Sha256};
    let schematic = include_bytes!("../../../../../test_data/EDGECASE_PISTION.schem");
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../test_data/piston-repair/java-piston-oscillator-trace.json"
    ))
    .unwrap();
    assert_eq!(reference["version"], "1.21.5");
    assert_eq!(
        reference["server_sha1"],
        "e6ec2f64e6080b9b5d9b471b291c33cc7f509733"
    );
    assert_eq!(
        reference["schematic_sha256"],
        format!("{:x}", Sha256::digest(schematic))
    );
    let cb = load_schematic(std::io::Cursor::new(schematic)).unwrap();
    let expected = reference["trace"].as_array().unwrap();
    for stepping in ["game", "nano", "pico"] {
        let mut world = empty_world();
        paste_clipboard(
            &mut world,
            &cb,
            BlockPos::new(40 + cb.offset_x, 30 + cb.offset_y, 40 + cb.offset_z),
            false,
        );
        for _ in 0..8 {
            world.tick_interpreted();
        }
        let source = BlockPos::new(40, 31, 40);
        crate::interaction::destroy(world.get_block(source), &mut world, source);
        // Dust changed from a line to a cross, but the piston watched by the
        // observer has not changed yet. Only its later retraction starts a pulse.
        assert!(!world.pending_tick_at(BlockPos::new(40, 32, 42)));
        for (tick, sample) in expected.iter().enumerate() {
            let piston_state = |z| match world.get_block(BlockPos::new(40, 31, z)) {
                Block::Piston { piston } if piston.extended => "sticky_piston[extended=true]",
                Block::Piston { .. } => "sticky_piston[extended=false]",
                Block::MovingPiston { .. } => "moving_piston",
                block => panic!("unexpected piston state {block:?}"),
            };
            let observer_powered = |z| matches!(world.get_block(BlockPos::new(40, 32, z)), Block::Observer { observer } if observer.powered);
            let wire_state = |z| {
                let block = world.get_block(BlockPos::new(40, 31, z));
                let Block::RedstoneWire { wire } = block else {
                    panic!("missing wire")
                };
                serde_json::json!([
                    wire.power,
                    block.property("north").unwrap(),
                    block.property("south").unwrap(),
                    block.property("east").unwrap(),
                    block.property("west").unwrap()
                ])
            };
            let actual = serde_json::json!([
                piston_state(42),
                piston_state(46),
                observer_powered(42),
                observer_powered(46),
                wire_state(41),
                wire_state(45)
            ]);
            assert_eq!(
                &actual, sample,
                "Java oscillator trace at tick {tick} with {stepping} stepping"
            );
            let target_tick = world.piston_state().logical_tick + 1;
            for _ in 0..256 {
                match stepping {
                    "game" => world.tick_interpreted(),
                    "nano" => world.nanotick_advance(1),
                    "pico" => world.picotick_advance(1),
                    _ => unreachable!(),
                }
                if world.piston_state().logical_tick == target_tick
                    && world.piston_state().phase == mchprs_world::AdvancePhase::BetweenTicks
                {
                    break;
                }
            }
            assert_eq!(world.piston_state().logical_tick, target_tick);
            assert_eq!(
                world.piston_state().phase,
                mchprs_world::AdvancePhase::BetweenTicks
            );
        }
    }
}

#[test]
fn redstone_update_edgecase_spits_then_recaptures_block_like_java_1_21_5() {
    use mchprs_blocks::blocks::RotateAmt;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    #[derive(Debug, Deserialize)]
    struct Sample {
        name: String,
        properties: BTreeMap<String, String>,
    }
    #[derive(Deserialize)]
    struct Case {
        rotation: u32,
        positions: Vec<[i32; 3]>,
        trace: Vec<Vec<Sample>>,
    }
    #[derive(Deserialize)]
    struct Reference {
        version: String,
        server_sha1: String,
        schematic_sha256: String,
        cases: Vec<Case>,
    }

    let schematic =
        include_bytes!("../../../../../test_data/MCHPRS_REDSTONE_UPDATE_EDGECASE.schem");
    let reference: Reference = serde_json::from_str(include_str!(
        "../../../../../test_data/piston-repair/java-redstone-update-edgecase.json"
    ))
    .unwrap();
    assert_eq!(reference.version, "1.21.5");
    assert_eq!(
        reference.server_sha1,
        "e6ec2f64e6080b9b5d9b471b291c33cc7f509733"
    );
    assert_eq!(
        reference.schematic_sha256,
        format!("{:x}", Sha256::digest(schematic))
    );
    assert_eq!(
        reference
            .cases
            .iter()
            .map(|case| case.rotation)
            .collect::<Vec<_>>(),
        [0, 90, 180, 270]
    );
    let original = load_schematic(std::io::Cursor::new(schematic)).unwrap();
    assert_eq!(
        (original.size_x, original.size_y, original.size_z),
        (7, 8, 7)
    );

    for case in reference.cases {
        let rotate = |mut pos: BlockPos| {
            for _ in 0..case.rotation / 90 {
                pos = BlockPos::new(6 - pos.z, pos.y, pos.x);
            }
            pos
        };
        let absolute = |pos: BlockPos| {
            let pos = rotate(pos);
            BlockPos::new(40 + pos.x, 30 + pos.y, 40 + pos.z)
        };
        let mut cb = original.clone();
        for y in 0..8 {
            for z in 0..7 {
                for x in 0..7 {
                    let mut block =
                        Block::from_id(original.data.get_entry((y * 49 + z * 7 + x) as usize));
                    for _ in 0..case.rotation / 90 {
                        block.rotate(RotateAmt::Rotate90);
                    }
                    let pos = rotate(BlockPos::new(x, y, z));
                    cb.data
                        .set_entry((pos.y * 49 + pos.z * 7 + pos.x) as usize, block.get_id());
                }
            }
        }
        cb.block_entities = original
            .block_entities
            .iter()
            .map(|(&pos, entity)| (rotate(pos), entity.clone()))
            .collect();

        for stepping in ["game", "nano", "pico"] {
            let mut world = empty_world();
            paste_clipboard(
                &mut world,
                &cb,
                BlockPos::new(40 + cb.offset_x, 30 + cb.offset_y, 40 + cb.offset_z),
                false,
            );
            for _ in 0..8 {
                world.tick_interpreted();
            }
            let trigger = absolute(BlockPos::new(6, 5, 6));
            assert_eq!(world.get_block(trigger), Block::RedstoneBlock {});
            crate::interaction::destroy(world.get_block(trigger), &mut world, trigger);

            assert_eq!(case.trace.len(), 49);
            for (tick, samples) in case.trace.iter().enumerate() {
                assert_eq!(samples.len(), case.positions.len());
                for (local, sample) in case.positions.iter().zip(samples) {
                    // Captured positions and properties are already rotated.
                    let pos = BlockPos::new(40 + local[0], 30 + local[1], 40 + local[2]);
                    let block = world.get_block(pos);
                    let context = format!(
                        "tick {tick}, rotation {}, {stepping} stepping, {pos:?}",
                        case.rotation
                    );
                    assert_eq!(block.get_name(), sample.name, "{context}");
                    for (key, value) in &sample.properties {
                        assert_eq!(
                            block.property(key),
                            Some(value.as_str()),
                            "{context}: {key}"
                        );
                    }
                }
                // This release is expected, not a wire-update failure. Keep the
                // actual payload cells in the check so base motion alone cannot
                // hide a permanently lost block or a suppressed vanilla spit.
                let front = absolute(BlockPos::new(2, 3, 2));
                let ahead = absolute(BlockPos::new(2, 2, 2));
                match tick {
                    5 => {
                        assert_eq!(world.get_block(front), Block::Air);
                        assert_eq!(world.get_block(ahead), Block::RedstoneBlock {});
                    }
                    9 => {
                        assert_eq!(world.get_block(ahead), Block::Air);
                        assert!(
                            matches!(world.get_block_entity(front), Some(BlockEntity::MovingPiston(entity)) if !entity.extending && !entity.source && entity.block_state == Block::RedstoneBlock {}.get_id())
                        );
                    }
                    11 => assert_eq!(world.get_block(front), Block::RedstoneBlock {}),
                    _ => {}
                }
                if tick + 1 == case.trace.len() {
                    break;
                }
                let target_tick = world.piston_state().logical_tick + 1;
                for _ in 0..256 {
                    match stepping {
                        "game" => world.tick_interpreted(),
                        "nano" => world.nanotick_advance(1),
                        "pico" => world.picotick_advance(1),
                        _ => unreachable!(),
                    }
                    if world.piston_state().logical_tick == target_tick
                        && world.piston_state().phase == mchprs_world::AdvancePhase::BetweenTicks
                    {
                        break;
                    }
                }
                assert_eq!(world.piston_state().logical_tick, target_tick);
                assert_eq!(
                    world.piston_state().phase,
                    mchprs_world::AdvancePhase::BetweenTicks
                );
            }
        }
    }
}

#[test]
fn observer_piston_feedback_matches_java_for_block_and_dust_triggers() {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Case {
        facing: String,
        dust: bool,
        hold: usize,
        trace: Vec<(String, Option<String>, String)>,
    }
    #[derive(Deserialize)]
    struct Reference {
        version: String,
        server_sha1: String,
        cases: Vec<Case>,
    }
    let reference: Reference = serde_json::from_str(include_str!(
        "../../../../../test_data/piston-repair/java-observer-piston-feedback.json"
    ))
    .unwrap();
    assert_eq!(reference.version, "1.21.5");
    assert_eq!(
        reference.server_sha1,
        "e6ec2f64e6080b9b5d9b471b291c33cc7f509733"
    );
    assert_eq!(reference.cases.len(), 16);

    for facing in [
        BlockFacing::East,
        BlockFacing::South,
        BlockFacing::West,
        BlockFacing::North,
    ] {
        for stepping in ["game", "nano", "pico"] {
            for case in &reference.cases {
                let mut world = empty_world();
                let pos = base();
                let observer_pos = pos.offset(BlockFace::Top);
                let behind = pos.offset(BlockFace::from(facing).opposite());
                let source = if case.dust {
                    behind.offset(BlockFace::from(facing).opposite())
                } else {
                    behind
                };
                world.set_block(
                    pos,
                    Block::Piston {
                        piston: piston(facing, false, false),
                    },
                );
                world.set_block(
                    observer_pos,
                    Block::Observer {
                        observer: RedstoneObserver {
                            facing: match case.facing.as_str() {
                                "down" => BlockFacing::Down, // Watches piston; red output dot points up.
                                "up" => BlockFacing::Up,
                                _ => panic!("unexpected observer facing"),
                            },
                            powered: false,
                        },
                    },
                );
                world.set_block(observer_pos.offset(BlockFace::Top), Block::Stone {});
                if case.dust {
                    world.set_block(behind.offset(BlockFace::Bottom), Block::Stone {});
                    crate::interaction::place_in_world(
                        Block::RedstoneWire {
                            wire: Default::default(),
                        },
                        &mut world,
                        behind,
                        &None,
                    );
                }
                let advance = |world: &mut PlotWorld| {
                    let target = world.piston_state().logical_tick + 1;
                    for _ in 0..256 {
                        match stepping {
                            "game" => world.tick_interpreted(),
                            "nano" => world.nanotick_advance(1),
                            "pico" => world.picotick_advance(1),
                            _ => unreachable!(),
                        }
                        if world.piston_state().logical_tick == target
                            && world.piston_state().phase
                                == mchprs_world::AdvancePhase::BetweenTicks
                        {
                            break;
                        }
                    }
                    assert_eq!(world.piston_state().logical_tick, target);
                    assert_eq!(
                        world.piston_state().phase,
                        mchprs_world::AdvancePhase::BetweenTicks
                    );
                };
                for _ in 0..8 {
                    advance(&mut world);
                }
                crate::interaction::place_in_world(
                    Block::RedstoneBlock {},
                    &mut world,
                    source,
                    &None,
                );
                // Placement requests piston movement, but has not changed the
                // watched piston yet. A diagonal power recheck must not start
                // the observer early and quasi-power the piston on removal.
                assert!(
                    !world.pending_tick_at(observer_pos),
                    "premature observer pulse: {facing:?}, dust={}",
                    case.dust
                );
                assert!(!extended(&world, pos));
                for _ in 0..case.hold {
                    advance(&mut world);
                }
                crate::interaction::destroy(world.get_block(source), &mut world, source);
                assert_eq!(case.trace.len(), 33);
                for (tick, expected) in case.trace.iter().enumerate() {
                    let block = world.get_block(pos);
                    let observer = world.get_block(observer_pos);
                    let actual = (
                        block.get_name().to_owned(),
                        block.property("extended").map(str::to_owned),
                        observer.property("powered").unwrap().to_owned(),
                    );
                    assert_eq!(&actual, expected, "Java feedback at tick {tick}: {facing:?}, observer={}, dust={}, hold={}, {stepping} stepping", case.facing, case.dust, case.hold);
                    if tick + 1 < case.trace.len() {
                        advance(&mut world);
                    }
                }
            }
        }
    }
}

#[test]
fn containers_cannot_be_pushed_or_pulled_in_any_state() {
    let mut world = empty_world();
    let pos = base();
    let front = pos.offset(BlockFace::East);
    let ahead = front.offset(BlockFace::East);
    for id in (19431..=19442).chain(10034..=10043).chain(4358..=4365) {
        let block = Block::from_id(id);
        world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
        start(&mut world, pos, BlockFacing::East, block);
        settle(&mut world);
        assert!(!extended(&world, pos), "container state {id} was pushed");
        assert_eq!(world.get_block(front), block);
        assert!(matches!(
            world.get_block_entity(front),
            Some(BlockEntity::Container { .. })
        ));
        world.set_block(front, Block::Air);
        world.set_block(ahead, block);
        world.set_block(
            pos,
            Block::Piston {
                piston: piston(BlockFacing::East, true, true),
            },
        );
        world.set_block(
            front,
            Block::PistonHead {
                head: RedstonePistonHead {
                    facing: BlockFacing::East,
                    sticky: true,
                    short: false,
                },
            },
        );
        world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
        crate::redstone::update(world.get_block(pos), &mut world, pos, None);
        settle(&mut world);
        let actual = world.get_block(ahead);
        // Neighbor notifications may legitimately unlock a hopper, without moving it.
        match (block, actual) {
            (Block::Hopper { facing, .. }, Block::Hopper { facing: actual, .. }) => {
                assert_eq!(facing, actual)
            }
            _ => assert_eq!(actual, block, "container state {id} was pulled"),
        }
        assert!(matches!(
            world.get_block_entity(ahead),
            Some(BlockEntity::Container { .. })
        ));
        world.set_block(ahead, Block::Air);
    }
}
fn base() -> BlockPos {
    BlockPos::new(40, 30, 40)
}
fn named(name: &str) -> Block {
    Block::from_name(name).unwrap()
}
fn piston(facing: BlockFacing, sticky: bool, extended: bool) -> RedstonePiston {
    RedstonePiston {
        facing,
        sticky,
        extended,
    }
}
fn start(world: &mut PlotWorld, pos: BlockPos, facing: BlockFacing, payload: Block) {
    world.set_block(
        pos,
        Block::Piston {
            piston: piston(facing, true, false),
        },
    );
    world.set_block(pos.offset(facing.into()), payload);
    let power_face = if facing == BlockFacing::Down {
        BlockFace::North
    } else {
        BlockFace::Bottom
    };
    world.set_block(pos.offset(power_face), Block::RedstoneBlock {});
    crate::redstone::update(world.get_block(pos), world, pos, None);
    world.picotick_advance(1);
}
fn settle(world: &mut PlotWorld) {
    for _ in 0..8 {
        world.tick_interpreted();
    }
}
fn extended(world: &PlotWorld, pos: BlockPos) -> bool {
    matches!(
        world.get_block(pos),
        Block::Piston {
            piston: RedstonePiston { extended: true, .. }
        }
    )
}
#[test]
fn movable_trapdoor_is_preserved() {
    let mut world = empty_world();
    let pos = base();
    let block = named("iron_trapdoor");
    start(&mut world, pos, BlockFacing::East, block);
    settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!("TRAPDOOR destination={:?}", world.get_block(target));
    assert!(matches!(
        world.get_block(target),
        Block::IronTrapdoor { .. }
    ));
}

#[test]
fn movement_cannot_lose_payload_at_plot_boundary() {
    let mut world = empty_world();
    let pos = BlockPos::new(254, 30, 40);
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    println!(
        "PLOT_EDGE base={:?} last_cell={:?}",
        world.get_block(pos),
        world.get_block(pos.offset(BlockFace::East))
    );
    assert_eq!(
        world.get_block(pos.offset(BlockFace::East)),
        Block::Stone {},
        "An isolated plot must reject a move to inaccessible storage"
    );
    assert!(!extended(&world, pos));
}

#[test]
fn movement_cannot_lose_payload_at_height_boundary() {
    let mut world = empty_world();
    let pos = BlockPos::new(40, 254, 40);
    start(&mut world, pos, BlockFacing::Up, Block::Stone {});
    settle(&mut world);
    println!(
        "HEIGHT_EDGE base={:?} last_cell={:?}",
        world.get_block(pos),
        world.get_block(pos.offset(BlockFace::Top))
    );
    assert_eq!(world.get_block(pos.offset(BlockFace::Top)), Block::Stone {});
    assert!(!extended(&world, pos));
}

#[test]
fn completion_cannot_recreate_replaced_base() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    world.set_block(pos, Block::GoldBlock {});
    settle(&mut world);
    println!("REPLACED_BASE current={:?}", world.get_block(pos));
    assert_eq!(
        world.get_block(pos),
        Block::GoldBlock {},
        "Java moving-entity completion never blindly rewrites a neighboring base"
    );
}

#[test]
fn removing_base_removes_stationary_head() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    crate::interaction::destroy(world.get_block(pos), &mut world, pos);
    println!(
        "ORPHAN_HEAD current={:?}",
        world.get_block(pos.offset(BlockFace::East))
    );
    assert_eq!(world.get_block(pos.offset(BlockFace::East)), Block::Air);
}

#[test]
fn breaking_stationary_head_removes_base() {
    let mut world = empty_world();
    let pos = base();
    let head = pos.offset(BlockFace::East);
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    crate::interaction::destroy(world.get_block(head), &mut world, head);
    println!(
        "BROKEN_HEAD base={:?} head={:?}",
        world.get_block(pos),
        world.get_block(head)
    );
    assert_eq!(world.get_block(pos), Block::Air);
}

#[test]
fn mismatched_head_does_not_mark_base_extended() {
    let mut world = empty_world();
    let pos = base();
    let invalid_head = Block::PistonHead {
        head: RedstonePistonHead {
            facing: BlockFacing::North,
            sticky: false,
            short: false,
        },
    };
    start(&mut world, pos, BlockFacing::East, invalid_head);
    settle(&mut world);
    println!("MISMATCHED_HEAD base={:?}", world.get_block(pos));
    assert!(!extended(&world, pos));
}

#[test]
fn moving_payload_has_its_own_destination_entity() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!(
        "MOVING_DESTINATION block={:?} entity={:?}",
        world.get_block(target),
        world.get_block_entity(target)
    );
    assert!(matches!(
        world.get_block(target),
        Block::MovingPiston { .. }
    ));
    assert!(
        matches!(world.get_block_entity(target), Some(BlockEntity::MovingPiston(e))
        if !e.source && e.block_state == Block::Stone {}.get_id())
    );
}

#[test]
fn normal_retraction_uses_moving_source_at_base() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
    crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    world.picotick_advance(1);
    println!(
        "RETRACT_SOURCE base={:?} entity={:?}",
        world.get_block(pos),
        world.get_block_entity(pos)
    );
    assert!(matches!(world.get_block(pos), Block::MovingPiston { .. }));
    assert!(
        matches!(world.get_block_entity(pos), Some(BlockEntity::MovingPiston(e)) if !e.extending && e.source)
    );
}

#[test]
fn movement_progress_advances_before_completion() {
    let mut world = empty_world();
    let pos = base();
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    world.tick_interpreted();
    let head = pos.offset(BlockFace::East);
    println!("PROGRESS entity={:?}", world.get_block_entity(head));
    assert!(world
        .piston_state()
        .motions
        .iter()
        .any(|m| m.pos == head && m.progress == 0.5));
}

#[test]
fn half_progress_is_exactly_representable() {
    let mut entity = MovingPistonEntity::default();
    entity.set_progress(0.5);
    println!(
        "HALF_PROGRESS encoded={} decoded={}",
        entity.progress,
        entity.get_progress()
    );
    assert_eq!(entity.get_progress(), 0.5);
}

#[test]
fn moved_powered_observer_does_not_stay_powered_forever() {
    let mut world = empty_world();
    let pos = base();
    // A powered observer normally has an outstanding pulse-off tick at its old position.
    world.set_block(pos.offset(BlockFace::East), world_observer());
    world.schedule_half_tick(
        pos.offset(BlockFace::East),
        2,
        mchprs_world::TickPriority::Normal,
    );
    start(
        &mut world,
        pos,
        BlockFacing::East,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: true,
            },
        },
    );
    settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    println!("MOVED_OBSERVER destination={:?}", world.get_block(target));
    assert!(matches!(
        world.get_block(target),
        Block::Observer {
            observer: RedstoneObserver { powered: false, .. }
        }
    ));
}

#[test]
fn old_observer_tick_cannot_complete_new_moving_head() {
    let mut world = empty_world();
    let pos = base();
    let head = pos.offset(BlockFace::East);
    world.set_block(
        head,
        Block::Observer {
            observer: RedstoneObserver {
                facing: BlockFacing::Down,
                powered: true,
            },
        },
    );
    world.schedule_half_tick(head, 1, mchprs_world::TickPriority::Normal);
    start(&mut world, pos, BlockFacing::East, world_observer());
    world.tick_interpreted();
    println!(
        "OLD_TICK head_after_one_game_tick={:?}",
        world.get_block(head)
    );
    assert!(
        matches!(world.get_block(head), Block::MovingPiston { .. }),
        "Java scheduled block ticks retain their block type and reject a replacement type"
    );
}

#[test]
fn moved_waterlogged_stairs_are_not_still_waterlogged() {
    let mut world = empty_world();
    let pos = base();
    let mut block = named("oak_stairs");
    block.set_properties([("waterlogged", "true")].into_iter().collect());
    assert_eq!(
        block.properties().get("waterlogged").map(String::as_str),
        Some("true")
    );
    start(&mut world, pos, BlockFacing::East, block);
    settle(&mut world);
    let target = pos.offset(BlockFace::East).offset(BlockFace::East);
    let moved = world.get_block(target);
    println!(
        "WATERLOGGED destination={:?} properties={:?}",
        moved,
        moved.properties()
    );
    assert_eq!(moved.get_name(), "oak_stairs");
    assert_eq!(
        moved.properties().get("waterlogged").map(String::as_str),
        Some("false"),
        "Java moving-entity completion clears waterlogging"
    );
}

#[test]
fn moving_support_breaks_torch_above_old_payload() {
    let mut world = empty_world();
    let pos = base();
    let torch = pos.offset(BlockFace::East).offset(BlockFace::Top);
    world.set_block(torch, Block::RedstoneTorch { lit: true });
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    println!("SUPPORT_REMOVED torch={:?}", world.get_block(torch));
    assert_eq!(
        world.get_block(torch),
        Block::Air,
        "Java shape updates remove a torch when its support becomes a horizontal piston head"
    );
}

#[test]
fn dust_on_downward_piston_survives_repeated_extension_and_retraction() {
    use mchprs_blocks::blocks::RedstoneWire;
    for sticky in [false, true] {
        let mut world = empty_world();
        let pos = base();
        let dust = pos.offset(BlockFace::Top);
        let power = pos.offset(BlockFace::North);
        world.set_block(
            pos,
            Block::Piston {
                piston: piston(BlockFacing::Down, sticky, false),
            },
        );
        world.set_block(
            dust,
            Block::RedstoneWire {
                wire: RedstoneWire::default(),
            },
        );
        world.set_block(pos.offset(BlockFace::Bottom), Block::Stone {});
        for _ in 0..2 {
            for source in [Block::RedstoneBlock {}, Block::Air] {
                world.set_block(power, source);
                crate::redstone::update(world.get_block(pos), &mut world, pos, None);
                for phase in 0..80 {
                    world.picotick_advance(1);
                    assert!(
                        matches!(world.get_block(dust), Block::RedstoneWire { .. }),
                        "dust disappeared: sticky={sticky}, source={source:?}, phase={phase}, base={:?}",
                        world.get_block(pos)
                    );
                }
                assert_eq!(extended(&world, pos), source != Block::Air);
            }
        }
    }
}

#[test]
fn dust_does_not_attach_to_moving_heads_or_payloads() {
    use mchprs_blocks::blocks::{RedstoneMovingPiston, RedstoneWire};
    let pos = base();
    let dust = pos.offset(BlockFace::Top);
    let base_state = Block::Piston {
        piston: piston(BlockFacing::Down, false, false),
    }
    .get_id();
    for facing in BlockFace::values() {
        for (source, extending, block_state) in [
            (false, false, base_state),
            (false, true, base_state),
            (true, true, base_state),
            (true, false, Block::Stone {}.get_id()),
            (true, false, base_state),
        ] {
            let mut world = empty_world();
            world.set_block(
                pos,
                Block::MovingPiston {
                    moving: RedstoneMovingPiston {
                        facing: facing.into(),
                        sticky: false,
                    },
                },
            );
            world.set_block_entity(
                pos,
                BlockEntity::MovingPiston(MovingPistonEntity {
                    facing,
                    source,
                    extending,
                    block_state,
                    progress: 0,
                }),
            );
            world.set_block(
                dust,
                Block::RedstoneWire {
                    wire: RedstoneWire::default(),
                },
            );
            let expected =
                facing == BlockFace::Bottom && source && !extending && block_state == base_state;
            assert_eq!(
                crate::interaction::is_valid_position(world.get_block(dust), &world, dust),
                expected,
                "facing={facing:?}, source={source}, extending={extending}, state={block_state}"
            );
        }
    }
}
fn world_observer() -> Block {
    Block::Observer {
        observer: RedstoneObserver {
            facing: BlockFacing::Down,
            powered: true,
        },
    }
}

#[test]
fn control_single_stone_extension_all_six_directions() {
    for facing in [
        BlockFacing::Down,
        BlockFacing::Up,
        BlockFacing::North,
        BlockFacing::South,
        BlockFacing::West,
        BlockFacing::East,
    ] {
        let mut world = empty_world();
        let pos = base();
        start(&mut world, pos, facing, Block::Stone {});
        settle(&mut world);
        assert!(extended(&world, pos));
        assert_eq!(
            world.get_block(pos.offset(facing.into()).offset(facing.into())),
            Block::Stone {}
        );
    }
}

#[test]
fn control_front_power_is_excluded() {
    let mut world = empty_world();
    let pos = base();
    world.set_block(
        pos,
        Block::Piston {
            piston: piston(BlockFacing::East, true, false),
        },
    );
    world.set_block(pos.offset(BlockFace::East), Block::RedstoneBlock {});
    assert!(!super::should_piston_extend(&world, BlockFacing::East, pos));
}

#[test]
fn control_quasi_connected_power_detected_after_base_update() {
    let mut world = empty_world();
    let pos = base();
    world.set_block(
        pos,
        Block::Piston {
            piston: piston(BlockFacing::East, true, false),
        },
    );
    world.set_block(
        pos.offset(BlockFace::Top).offset(BlockFace::East),
        Block::RedstoneBlock {},
    );
    assert!(super::should_piston_extend(&world, BlockFacing::East, pos));
    assert!(!world.pending_tick_at(pos));
    crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    settle(&mut world);
    assert!(extended(&world, pos));
}

#[test]
fn extension_sets_base_state_during_event_and_does_not_schedule_a_lock() {
    let mut world = empty_world();
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    assert!(extended(&world, base()));
    assert!(matches!(
        world.get_block(base().offset(BlockFace::East)),
        Block::MovingPiston { .. }
    ));
    assert!(!world.scheduler().iter_entries().any(|e| e.pos == base()));
}

#[test]
fn power_off_request_ignores_unrelated_future_base_tick() {
    let mut world = empty_world();
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    settle(&mut world);
    world.schedule_half_tick(base(), 3, mchprs_world::TickPriority::Normal);
    world.set_block(base().offset(BlockFace::Bottom), Block::Air);
    crate::redstone::update(world.get_block(base()), &mut world, base(), None);
    assert!(world
        .piston_state()
        .events
        .iter()
        .any(|e| e.pos == base() && e.action == mchprs_world::PistonAction::Retract));
    world.picotick_advance(1);
    assert!(matches!(
        world.get_block(base()),
        Block::MovingPiston { .. }
    ));
}

#[test]
fn short_pulse_drops_but_completed_extension_pulls() {
    for movement_ticks in 0..=3 {
        let mut world = empty_world();
        let head = base().offset(BlockFace::East);
        let destination = head.offset(BlockFace::East);
        start(&mut world, base(), BlockFacing::East, Block::Stone {});
        for _ in 0..movement_ticks {
            world.tick_interpreted();
        }
        world.set_block(base().offset(BlockFace::Bottom), Block::Air);
        crate::redstone::update(world.get_block(base()), &mut world, base(), None);
        let expected = if movement_ticks < 3 {
            mchprs_world::PistonAction::RetractWithoutPull
        } else {
            mchprs_world::PistonAction::Retract
        };
        assert_eq!(world.piston_state().events.back().unwrap().action, expected);
        settle(&mut world);
        assert_eq!(
            world.get_block(destination),
            if movement_ticks < 3 {
                Block::Stone {}
            } else {
                Block::Air
            },
            "pulse {movement_ticks}"
        );
        assert_eq!(
            world.get_block(head),
            if movement_ticks < 3 {
                Block::Air
            } else {
                Block::Stone {}
            },
            "pulse {movement_ticks}"
        );
        assert!(matches!(world.get_block(base()), Block::Piston { piston } if !piston.extended));
    }
}

#[test]
fn push_chain_snapshots_preserve_overlapping_payloads() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    world.set_block(head.offset(BlockFace::East), Block::GoldBlock {});
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    settle(&mut world);
    assert_eq!(
        world.get_block(head.offset(BlockFace::East)),
        Block::Stone {}
    );
    assert_eq!(
        world.get_block(head.offset(BlockFace::East).offset(BlockFace::East)),
        Block::GoldBlock {}
    );
}

#[test]
fn long_payload_lines_spill_and_preserve_every_block() {
    let mut world = empty_world();
    let pos = base();
    let length = 32;
    for offset in 1..=length {
        world.set_block(
            BlockPos::new(pos.x + offset, pos.y, pos.z),
            if offset % 2 == 0 {
                Block::GoldBlock {}
            } else {
                Block::Stone {}
            },
        );
    }
    start(&mut world, pos, BlockFacing::East, Block::Stone {});
    settle(&mut world);
    assert!(extended(&world, pos));
    for offset in 1..=length {
        assert_eq!(
            world.get_block(BlockPos::new(pos.x + offset + 1, pos.y, pos.z)),
            if offset % 2 == 0 {
                Block::GoldBlock {}
            } else {
                Block::Stone {}
            },
            "payload {offset}"
        );
    }
}

#[test]
fn canceled_event_rechecks_power_and_is_deduplicated() {
    let mut world = empty_world();
    let pos = base();
    world.set_block(
        pos,
        Block::Piston {
            piston: piston(BlockFacing::East, true, false),
        },
    );
    world.set_block(pos.offset(BlockFace::Bottom), Block::RedstoneBlock {});
    for _ in 0..5 {
        crate::redstone::update(world.get_block(pos), &mut world, pos, None);
    }
    assert_eq!(world.piston_state().events.len(), 1);
    world.set_block(pos.offset(BlockFace::Bottom), Block::Air);
    world.picotick_advance(1);
    assert!(!extended(&world, pos));
    assert!(world.piston_state().motions.is_empty());
}

#[test]
fn replacement_payload_cancels_old_completion() {
    let mut world = empty_world();
    let dest = base().offset(BlockFace::East).offset(BlockFace::East);
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    world.tick_interpreted();
    world.set_block(dest, Block::GoldBlock {});
    settle(&mut world);
    assert_eq!(world.get_block(dest), Block::GoldBlock {});
    assert!(world.get_block_entity(dest).is_none());
}

#[test]
fn breaking_moving_head_removes_base_without_resurrection() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    crate::interaction::destroy(world.get_block(head), &mut world, head);
    settle(&mut world);
    assert_eq!(world.get_block(head), Block::Air);
    assert_eq!(world.get_block(base()), Block::Air);
}

#[test]
fn breaking_base_during_extension_removes_owned_moving_head() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    crate::interaction::destroy(world.get_block(base()), &mut world, base());
    assert_eq!(world.get_block(head), Block::Air);
    settle(&mut world);
    assert_eq!(world.get_block(head), Block::Air);
    assert_eq!(world.get_block(base()), Block::Air);
}

#[test]
fn interrupted_waterlogged_payload_retains_waterlogging() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    let mut payload = named("oak_stairs");
    payload.set_properties([("waterlogged", "true")].into_iter().collect());
    start(&mut world, base(), BlockFacing::East, payload);
    world.set_block(base().offset(BlockFace::Bottom), Block::Air);
    crate::redstone::update(world.get_block(base()), &mut world, base(), None);
    settle(&mut world);
    let dropped = world.get_block(head.offset(BlockFace::East));
    assert_eq!(dropped.get_name(), "oak_stairs");
    assert_eq!(
        dropped.properties().get("waterlogged").map(String::as_str),
        Some("true")
    );
}

#[test]
fn game_nano_and_pico_steps_share_event_and_movement_order() {
    use mchprs_world::AdvancePhase;
    let mut worlds = [empty_world(), empty_world(), empty_world()];
    for world in &mut worlds {
        start(world, base(), BlockFacing::East, Block::Stone {});
    }
    for game_tick in 1..=4 {
        worlds[0].tick_interpreted();
        for mode in 1..=2 {
            for _ in 0..30 {
                if worlds[mode].piston_state().logical_tick == game_tick
                    && worlds[mode].piston_state().phase == AdvancePhase::BetweenTicks
                {
                    break;
                }
                if mode == 1 {
                    worlds[mode].nanotick_advance(1);
                } else {
                    worlds[mode].picotick_advance(1);
                }
            }
            assert_eq!(worlds[mode].piston_state().logical_tick, game_tick);
            assert_eq!(
                worlds[mode].piston_state().phase,
                AdvancePhase::BetweenTicks
            );
            assert_eq!(
                worlds[mode].piston_state().events,
                worlds[0].piston_state().events
            );
            for n in 0..=2 {
                let p = BlockPos::new(base().x + n, base().y, base().z);
                assert_eq!(worlds[mode].get_block(p), worlds[0].get_block(p));
            }
            let snapshot = |w: &PlotWorld| {
                w.piston_state()
                    .motions
                    .iter()
                    .map(|m| {
                        (
                            m.pos,
                            m.identity,
                            m.progress,
                            m.previous_progress,
                            m.last_tick,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(snapshot(&worlds[mode]), snapshot(&worlds[0]));
        }
    }
}

#[test]
fn chunk_nbt_uses_previous_progress_like_java() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    for previous in [0.0, 0.5] {
        world.tick_interpreted();
        let nbt = world.get_block_entity(head).unwrap().to_nbt(false).unwrap();
        assert!(matches!(nbt.get("progress"),Some(nbt::Value::Float(p)) if *p==previous));
    }
}

#[test]
fn removing_vertical_base_updates_support_at_removed_head() {
    let mut world = empty_world();
    start(&mut world, base(), BlockFacing::Up, Block::Air);
    settle(&mut world);
    let torch = base().offset(BlockFace::Top).offset(BlockFace::Top);
    world.set_block(torch, Block::RedstoneTorch { lit: true });
    assert!(crate::interaction::is_valid_position(
        world.get_block(torch),
        &world,
        torch
    ));
    crate::interaction::destroy(world.get_block(base()), &mut world, base());
    assert_eq!(world.get_block(torch), Block::Air);
}

#[test]
fn moved_observer_preserves_valid_pending_tick_at_destination() {
    let mut world = empty_world();
    let head = base().offset(BlockFace::East);
    let destination = head.offset(BlockFace::East);
    world.set_block(destination, world_observer());
    world.schedule_half_tick(destination, 6, mchprs_world::TickPriority::Normal);
    start(&mut world, base(), BlockFacing::East, world_observer());
    for _ in 0..3 {
        world.tick_interpreted();
    }
    assert!(matches!(world.get_block(destination),Block::Observer {observer} if observer.powered));
    for _ in 0..3 {
        world.tick_interpreted();
    }
    assert!(matches!(world.get_block(destination),Block::Observer {observer} if !observer.powered));
}

#[test]
fn completion_runs_once_and_repeated_stale_work_preserves_payload() {
    let mut world = empty_world();
    let destination = base().offset(BlockFace::East).offset(BlockFace::East);
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    let identity = world
        .piston_state()
        .motions
        .iter()
        .find(|m| m.pos == destination)
        .unwrap()
        .identity;
    settle(&mut world);
    for _ in 0..3 {
        super::tick_motion(&mut world, destination, identity);
    }
    assert_eq!(world.get_block(destination), Block::Stone {});
    assert!(world.get_block_entity(destination).is_none());
}

#[test]
fn replacing_retracting_base_does_not_resurrect_it() {
    let mut world = empty_world();
    start(&mut world, base(), BlockFacing::East, Block::Stone {});
    settle(&mut world);
    world.set_block(base().offset(BlockFace::Bottom), Block::Air);
    crate::redstone::update(world.get_block(base()), &mut world, base(), None);
    world.picotick_advance(1);
    world.set_block(base(), Block::GoldBlock {});
    settle(&mut world);
    assert_eq!(world.get_block(base()), Block::GoldBlock {});
}

#[test]
fn malformed_moving_progress_is_rejected() {
    let entity = BlockEntity::MovingPiston(MovingPistonEntity {
        block_state: Block::Stone {}.get_id(),
        ..Default::default()
    });
    for progress in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        let mut nbt = entity.to_nbt(false).unwrap();
        nbt.insert("progress", nbt::Value::Float(progress)).unwrap();
        assert!(BlockEntity::from_nbt(&nbt.content).is_err());
    }
}
