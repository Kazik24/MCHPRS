use super::*;

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn compiled_plate_inputs_remain_active_and_validate_the_batch_before_mutation() {
    let pos = BlockPos::new(21, 4, 30);
    let lamp = pos.offset(BlockFace::Top);
    for llvm in [false, true] {
        let mut input = snapshot();
        input.min = BlockPos::new(20, 3, 29);
        input.blocks = vec![
            (pos.offset(BlockFace::Bottom), Block::Stone {}),
            (pos, Block::StonePressurePlate { powered: false }),
            (lamp, Block::RedstoneLamp { lit: false }),
        ];
        let mut session = Session::new(input).unwrap();
        session.compile(CompilerOptions { llvm, ..Default::default() }).unwrap();
        assert!(
            session
                .advance_with_inputs(1, &[Input::Plate(pos, true), Input::Plate(BlockPos::new(20, 4, 30), true)])
                .is_err()
        );
        assert_eq!(session.ticks(), 0);
        assert_eq!(session.get(pos).unwrap().pressure_plate_powered(), Some(false));
        session
            .advance_with_inputs(21, &[Input::Plate(pos, true), Input::Plate(pos, true)])
            .unwrap();
        assert_eq!(
            session.get(pos).unwrap().pressure_plate_powered(),
            Some(true),
            "plates must not acquire the button release timer"
        );
        assert_eq!(session.get(lamp).unwrap(), Block::RedstoneLamp { lit: true });
        assert!(session.compiler.is_active());
        session.advance_with_inputs(4, &[Input::Plate(pos, false)]).unwrap();
        assert_eq!(session.get(pos).unwrap().pressure_plate_powered(), Some(false));
        assert_eq!(session.get(lamp).unwrap(), Block::RedstoneLamp { lit: false });
        assert!(session.compiler.is_active());
    }
}

fn snapshot() -> Snapshot {
    Snapshot {
        min: BlockPos::new(20, 4, 30),
        max: BlockPos::new(22, 5, 31),
        blocks: vec![],
        entities: vec![],
        ticks: vec![],
    }
}

#[test]
fn snapshot_validation_and_load_preserve_native_state() {
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    let comparator = Block::from_name("comparator").unwrap();
    input.blocks.push((pos, comparator));
    input.entities.push((pos, BlockEntity::Comparator { output_strength: 9 }));
    input.ticks.push(TickEntry {
        pos,
        ticks_left: 3,
        tick_priority: TickPriority::High,
        block_type: Some(comparator.registry_id()),
    });
    input.ticks.push(TickEntry {
        pos,
        ticks_left: 3,
        tick_priority: TickPriority::High,
        block_type: Some(comparator.registry_id()),
    });
    let mut session = Session::new(input).unwrap();
    assert_eq!(session.get(pos).unwrap(), comparator);
    assert_eq!(session.get(BlockPos::new(20, 4, 30)).unwrap(), Block::Air);
    assert!(matches!(
        session.world.get_block_entity(pos),
        Some(BlockEntity::Comparator { output_strength: 9 })
    ));
    let ticks: Vec<_> = session.world.scheduler().iter_entries().collect();
    assert_eq!(ticks.len(), 2);
    assert!(
        ticks
            .iter()
            .all(|tick| tick.ticks_left == 3 && tick.pos == pos && tick.tick_priority == TickPriority::High)
    );
    assert!(session.drain_changes().unwrap().is_empty());

    let mut duplicate = snapshot();
    duplicate.blocks = vec![(pos, comparator), (pos, Block::Air)];
    assert!(Session::new(duplicate).is_err());
    let mut invalid_tick = snapshot();
    invalid_tick.ticks.push(TickEntry {
        pos,
        ticks_left: 32,
        tick_priority: TickPriority::Normal,
        block_type: None,
    });
    assert!(Session::new(invalid_tick).is_err());
    let mut outside = snapshot();
    outside.blocks.push((BlockPos::new(19, 4, 30), comparator));
    assert!(Session::new(outside).is_err());
    let mut wrong_entity = snapshot();
    wrong_entity.blocks.push((pos, Block::from_name("chest").unwrap()));
    wrong_entity.entities.push((pos, BlockEntity::Comparator { output_strength: 2 }));
    assert!(Session::new(wrong_entity).is_err());
}

#[test]
fn interpreter_controls_and_drain_use_plot_positions() {
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    input.blocks.push((pos, Block::from_name("lever").unwrap()));
    let mut session = Session::new(input).unwrap();
    session.use_block(pos).unwrap();
    assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if lever.powered));
    assert!(session.drain_changes().unwrap().iter().any(|(changed, _)| *changed == pos));
    assert!(session.drain_changes().unwrap().is_empty());
    assert!(session.use_block(BlockPos::new(20, 4, 30)).is_err());
    assert!(session.get(BlockPos::new(0, 0, 0)).is_err());
    session.advance(1).unwrap();
}

#[test]
fn compiled_session_accepts_io_settings_with_complete_projection() {
    let mut session = Session::new(snapshot()).unwrap();
    let options = CompilerOptions {
        io_only: true,
        ..Default::default()
    };
    session.compile(options).unwrap();
    assert_eq!(session.get(BlockPos::new(20, 4, 30)).unwrap(), Block::Air);
    session.advance(1).unwrap();
    assert!(session.drain_changes().unwrap().is_empty());
}

#[test]
fn compiled_piston_projection_and_reset_match_native() {
    use mchprs_blocks::{
        BlockDirection, BlockFacing,
        blocks::{Lever, LeverFace, RedstonePiston},
    };
    let lever = BlockPos::new(15, 15, 15);
    let base = lever.offset(BlockFace::East);
    let home = base.offset(BlockFace::East);
    let far = home.offset(BlockFace::East);
    let make_snapshot = || Snapshot {
        min: BlockPos::new(10, 10, 10),
        max: BlockPos::new(22, 20, 20),
        blocks: vec![
            (lever.offset(BlockFace::Bottom), Block::Stone {}),
            (
                lever,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            ),
            (
                base,
                Block::Piston {
                    piston: RedstonePiston {
                        facing: BlockFacing::East,
                        sticky: true,
                        extended: false,
                    },
                },
            ),
            (home, Block::RedstoneBlock),
        ],
        entities: vec![],
        ticks: vec![],
    };
    for reset_at in [None, Some(0), Some(1), Some(2), Some(3)] {
        let mut native = Session::new(make_snapshot()).unwrap();
        let mut compiled = Session::new(make_snapshot()).unwrap();
        compiled.compile(CompilerOptions::default()).unwrap();
        native.use_block(lever).unwrap();
        compiled.use_block(lever).unwrap();
        for tick in 0..6 {
            for pos in [base, home, far] {
                assert_eq!(
                    compiled.get(pos).unwrap(),
                    native.get(pos).unwrap(),
                    "tick {tick}, reset {reset_at:?}, {pos:?}"
                );
            }
            let native_changes: HashMap<_, _> = native.drain_changes().unwrap().into_iter().collect();
            let compiled_changes: HashMap<_, _> = compiled.drain_changes().unwrap().into_iter().collect();
            for pos in [base, home, far] {
                assert_eq!(
                    compiled_changes.get(&pos),
                    native_changes.get(&pos),
                    "changes at tick {tick}, reset {reset_at:?}, {pos:?}"
                );
            }
            if reset_at == Some(tick) {
                compiled.reset().unwrap();
            }
            native.advance(1).unwrap();
            compiled.advance(1).unwrap();
        }
    }
}

#[test]
fn compiled_reset_hands_pending_ticks_back_once() {
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    input.min = BlockPos::new(20, 3, 29);
    let block = Block::RedstoneLamp { lit: false };
    input.blocks.push((pos, block));
    input.ticks.push(TickEntry {
        pos,
        ticks_left: 4,
        tick_priority: TickPriority::Normal,
        block_type: Some(block.registry_id()),
    });
    let mut session = Session::new(input).unwrap();
    let before = session.pending_ticks().unwrap();
    let monitor = Arc::new(TaskMonitor::default());
    monitor.cancel();
    assert!(session.compile_with_monitor(CompilerOptions::default(), monitor).is_err());
    assert_eq!(session.pending_ticks().unwrap(), before);
    session.compile(CompilerOptions::default()).unwrap();
    assert!(session.pending_ticks().is_err());
    assert_eq!(session.world.scheduler().iter_entries().count(), 0);
    session.reset().unwrap();
    let ticks = session.pending_ticks().unwrap();
    assert_eq!(ticks.len(), 1);
    assert_eq!(ticks[0].pos, pos);
    assert_eq!(ticks[0].ticks_left, 4);
}

#[test]
fn power_and_ordered_controls_use_the_current_state() {
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    input.blocks.push((pos, Block::from_name("lever").unwrap()));
    let mut session = Session::new(input).unwrap();
    session.use_block(pos).unwrap();
    assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if lever.powered));
    session.use_block(pos).unwrap();
    assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if !lever.powered));
    session.advance(2).unwrap();
    assert_eq!(session.ticks(), 2);
    assert!(session.power(BlockPos::new(0, 0, 0), BlockFace::Top).is_err());
    assert!(session.drain_changes().unwrap().iter().any(|(changed, _)| *changed == pos));
    assert!(session.drain_changes().unwrap().is_empty());
}

#[test]
fn input_batch_validates_before_mutating_and_preserves_order() {
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    input.blocks.push((pos, Block::from_name("lever").unwrap()));
    let mut session = Session::new(input).unwrap();
    assert!(
        session
            .advance_with_inputs(1, &[Input::Use(pos), Input::Use(BlockPos::new(20, 4, 30))])
            .is_err()
    );
    assert_eq!(session.ticks(), 0);
    assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if !lever.powered));
    session.advance_with_inputs(1, &[Input::Use(pos), Input::Use(pos)]).unwrap();
    assert_eq!(session.ticks(), 1);
    assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if !lever.powered));
}

#[test]
fn unsupported_compile_keeps_the_headless_session_native() {
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    let block = Block::Piston {
        piston: mchprs_blocks::blocks::RedstonePiston {
            facing: mchprs_blocks::BlockFacing::East,
            sticky: false,
            extended: false,
        },
    };
    input.blocks.push((pos, block));
    input.blocks.push((pos.offset(BlockFace::East), Block::Stone {}));
    let mut session = Session::new(input).unwrap();
    assert!(session.compile(CompilerOptions::default()).is_err());
    session.advance(1).unwrap();
    assert_eq!(session.ticks(), 1);
    assert_eq!(session.get(pos).unwrap(), block);
}

#[test]
fn drain_reports_air_and_consumes_it_once() {
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    input.blocks.push((pos, Block::from_name("stone").unwrap()));
    let mut session = Session::new(input).unwrap();
    session.world.set_block(pos, Block::Air);
    assert!(
        session
            .drain_changes()
            .unwrap()
            .iter()
            .any(|(changed, state)| *changed == pos && *state == Block::get_id(Block::Air))
    );
    assert!(session.drain_changes().unwrap().is_empty());
}

#[test]
fn inspect_power_and_block_entity() {
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    input.blocks.push((pos, Block::RedstoneBlock {}));
    let session = Session::new(input).unwrap();
    assert_eq!(session.power(pos, BlockFace::Top).unwrap(), 15);
    assert!(session.block_entity(pos).unwrap().is_none());
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn binary_power_is_explicit_ordered_and_validated_on_both_executors() {
    let pos = BlockPos::new(21, 4, 30);
    for llvm in [false, true] {
        let mut input = snapshot();
        input.min.y = 3;
        input.blocks = vec![
            (pos.offset(BlockFace::Bottom), Block::Stone {}),
            (pos, Block::from_name("lever").unwrap()),
            (pos.offset(BlockFace::East), Block::RedstoneWire { wire: Default::default() }),
            (pos.offset(BlockFace::East).offset(BlockFace::Bottom), Block::Stone {}),
        ];
        let original = input.blocks.iter().map(|&(pos, block)| (pos, block.get_id())).collect::<Vec<_>>();
        let mut session = Session::new(input).unwrap();
        assert_eq!(session.original_blocks(), original);
        session
            .compile(CompilerOptions {
                llvm,
                io_only: true,
                ..Default::default()
            })
            .unwrap();
        assert!(
            session
                .validate_inputs(&[Input::Power(pos, true), Input::Power(BlockPos::new(20, 4, 30), true)])
                .is_err()
        );
        assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if !lever.powered));
        session
            .advance_with_inputs(1, &[Input::Power(pos, true), Input::Power(pos, true)])
            .unwrap();
        assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if lever.powered));
        assert!(matches!(session.get(pos.offset(BlockFace::East)).unwrap(), Block::RedstoneWire { wire } if wire.power == 15));
        session
            .advance_with_inputs(0, &[Input::Power(pos, false), Input::Power(pos, false)])
            .unwrap();
        assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if !lever.powered));
        assert!(matches!(session.get(pos.offset(BlockFace::East)).unwrap(), Block::RedstoneWire { wire } if wire.power == 0));
        assert_eq!(session.ticks(), 1);
        assert_eq!(session.original_blocks(), original);
    }
}
