use mchprs_blocks::{
    BlockDirection, BlockPos,
    blocks::{Block, Lever, LeverFace},
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
