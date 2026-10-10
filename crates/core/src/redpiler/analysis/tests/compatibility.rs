use super::*;
use rustc_hash::FxHashSet;

#[test]
fn notification_inventory_includes_absent_writer_heads_and_receiver_eligibility() {
    use crate::redpiler::analysis::ports::UpdateKind;
    let mut world = empty();
    let piston = RedstonePiston {
        facing: BlockFacing::Down,
        sticky: true,
        extended: true,
    };
    world.set_block(BASE, Block::Piston { piston });
    world.set_block(
        BASE.offset(BlockFace::Bottom),
        Block::PistonHead {
            head: piston.into(),
        },
    );
    world.set_block(BASE + BlockPos::new(0, -2, 0), Block::RedstoneBlock);
    for y in [0, -1] {
        world.set_block(
            BASE + BlockPos::new(-2, y, 0),
            Block::Piston {
                piston: RedstonePiston {
                    facing: BlockFacing::East,
                    sticky: false,
                    extended: false,
                },
            },
        );
    }
    let report = analyze_world(&world);
    let actor = report.pistons.iter().position(|p| p.pos == BASE).unwrap();
    let updates = &report.ports.pistons[actor].updates;
    for (y, requires_extended) in [(0, false), (-1, true)] {
        let source = BASE + BlockPos::new(-1, y, 0);
        assert_eq!(world.get_block(source), Block::Air);
        assert!(updates.iter().any(|u| u.source == source
            && u.kind == UpdateKind::AdjacentHeadChange
            && u.requires_extended == requires_extended));
    }
    let unique: FxHashSet<_> = updates
        .iter()
        .map(|u| (u.source, u.kind as u8, u.requires_extended))
        .collect();
    assert_eq!(unique.len(), updates.len());
}
