use super::*;
use crate::world::storage::Chunk;
use mchprs_blocks::block_entities::SignalStrength;
use mchprs_blocks::items::ItemStack;

#[test]
fn overlapping_stacks_use_one_source_and_restore_original_entities() {
    for air in [AirPolicy::Copy, AirPolicy::Ignore] {
        let mut world = PlotWorld::from_chunks(0, 0, vec![Chunk::empty(0, 0)], Default::default());
        let start = BlockPos::new(4, 30, 4);
        let end = BlockPos::new(6, 30, 4);
        world.set_block(start, Block::Stone {});
        for (x, kind, power) in [(5, ContainerType::Chest, 15), (7, ContainerType::Barrel, 3)] {
            let item = ItemStack::container_with_ss(kind, SignalStrength::new(power).unwrap());
            let block = Block::from_name(item.item_type.get_name()).unwrap();
            crate::interaction::place_in_world(
                block,
                &mut world,
                BlockPos::new(x, 30, 4),
                &item.nbt,
            );
        }
        world.set_block(BlockPos::new(8, 30, 4), Block::Sand {});
        let positions: Vec<_> = (4..=8).map(|x| BlockPos::new(x, 30, 4)).collect();
        let original = snapshot(&world, &positions);
        let undo = stack_prepared(
            &mut world,
            start,
            end,
            &[
                (BlockPos::new(5, 30, 4), BlockPos::new(7, 30, 4)),
                (BlockPos::new(6, 30, 4), BlockPos::new(8, 30, 4)),
            ],
            air,
        );
        assert_eq!(world.get_block(BlockPos::new(6, 30, 4)), Block::Stone {});
        assert_eq!(
            ContainerType::from_block(world.get_block(BlockPos::new(7, 30, 4))),
            Some(ContainerType::Chest)
        );
        assert_eq!(
            crate::redstone::comparator::get_override(
                world.get_block(BlockPos::new(7, 30, 4)),
                &world,
                BlockPos::new(7, 30, 4)
            ),
            15
        );
        let after = snapshot(&world, &positions);
        let redo = create_clipboard(&mut world, start, start, BlockPos::new(8, 30, 4));
        for clipboard in undo.clipboards.iter().rev() {
            paste_clipboard(&mut world, clipboard, undo.pos, false);
        }
        assert_eq!(snapshot(&world, &positions), original);
        paste_clipboard(&mut world, &redo, start, false);
        assert_eq!(snapshot(&world, &positions), after);
    }
}

fn snapshot(world: &PlotWorld, positions: &[BlockPos]) -> Vec<(u32, Option<Vec<u8>>)> {
    positions
        .iter()
        .map(|&position| {
            (
                world.get_block_raw(position),
                world
                    .get_block_entity(position)
                    .map(|entity| bincode::serialize(entity).unwrap()),
            )
        })
        .collect()
}
