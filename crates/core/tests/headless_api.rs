use mchprs_blocks::{
    BlockDirection, BlockPos,
    blocks::{Block, ButtonFace, Lever, LeverFace, StoneButton},
};
use mchprs_core::headless::{CompilerOptions, Input, Session, Snapshot};

#[test]
fn host_runs_a_compiled_lever_with_only_the_public_api() {
    let pos = BlockPos::new(5, 5, 5);
    let mut session = Session::new(Snapshot {
        min: BlockPos::new(4, 4, 4),
        max: BlockPos::new(6, 6, 6),
        blocks: vec![
            (BlockPos::new(5, 4, 5), Block::from_name("stone").unwrap()),
            (
                pos,
                Block::Lever {
                    lever: Lever::new(LeverFace::Floor, BlockDirection::North, false),
                },
            ),
        ],
        entities: vec![],
        ticks: vec![],
    })
    .unwrap();
    session.compile(CompilerOptions::default()).unwrap();
    session.advance_with_inputs(1, &[Input::Use(pos)]).unwrap();
    assert_eq!(session.ticks(), 1);
    assert!(matches!(session.get(pos).unwrap(), Block::Lever { lever } if lever.powered));
    assert!(session.drain_changes().unwrap().iter().any(|(changed, _)| *changed == pos));
    assert!(session.drain_changes().unwrap().is_empty());
}

#[test]
fn host_presses_a_compiled_button_through_the_public_api() {
    let pos = BlockPos::new(5, 5, 5);
    let mut session = Session::new(Snapshot {
        min: BlockPos::new(2, 2, 2),
        max: BlockPos::new(8, 8, 8),
        blocks: vec![
            (BlockPos::new(5, 4, 5), Block::from_name("stone").unwrap()),
            (
                pos,
                Block::StoneButton {
                    button: StoneButton::new(ButtonFace::Floor, BlockDirection::North, false),
                },
            ),
            (BlockPos::new(5, 6, 5), Block::RedstoneLamp { lit: false }),
        ],
        entities: vec![],
        ticks: vec![],
    })
    .unwrap();
    session.compile(CompilerOptions::default()).unwrap();
    session.advance_with_inputs(1, &[Input::Use(pos)]).unwrap();
    assert!(matches!(session.get(pos).unwrap(), Block::StoneButton { button } if button.powered));
    assert!(matches!(session.get(BlockPos::new(5, 6, 5)).unwrap(), Block::RedstoneLamp { lit: true }));
    session.advance_with_inputs(19, &[]).unwrap();
    assert!(matches!(session.get(pos).unwrap(), Block::StoneButton { button } if !button.powered));
}
