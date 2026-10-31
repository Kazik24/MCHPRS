use super::*;

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
fn compiled_session_requires_full_projection() {
    let mut session = Session::new(snapshot()).unwrap();
    let options = CompilerOptions {
        io_only: true,
        ..Default::default()
    };
    assert!(session.compile(options).is_err());
    assert_eq!(session.get(BlockPos::new(20, 4, 30)).unwrap(), Block::Air);
    session.compile(CompilerOptions::default()).unwrap();
    session.advance(1).unwrap();
    assert!(session.drain_changes().unwrap().is_empty());
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
    assert!(
        session
            .compile(CompilerOptions {
                io_only: true,
                ..Default::default()
            })
            .is_err()
    );
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
    use mchprs_blocks::block_entities::CommandBlockEntity;
    let pos = BlockPos::new(21, 4, 30);
    let mut input = snapshot();
    input.blocks.push((pos, Block::from_name("command_block").unwrap()));
    input.entities.push((
        pos,
        BlockEntity::CommandBlock(Box::new(CommandBlockEntity {
            command: "say test".into(),
            automatic: true,
            ..Default::default()
        })),
    ));
    let mut session = Session::new(input).unwrap();
    assert!(session.compile(CompilerOptions::default()).is_err());
    session.advance(1).unwrap();
    assert_eq!(session.ticks(), 1);
    assert_eq!(session.get(pos).unwrap(), Block::from_name("command_block").unwrap());
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
