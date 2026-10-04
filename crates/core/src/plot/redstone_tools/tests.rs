use super::*;
use crate::world::World;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType, SignalStrength};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::items::ItemStack;

#[test]
fn generated_actions_use_modern_protocol_and_fixed_commands() {
    let action = ResultAction::Teleport(BlockPos::new(1, 2, 3)).component("location");
    assert_eq!(action["click_event"]["command"], "/tp 1 3 3");
    assert!(action.get("clickEvent").is_none());
    let page = ResultAction::Page {
        kind: SearchKind::Signs,
        page: 2,
    }
    .component("next");
    assert_eq!(page["click_event"]["command"], "//signsearch -p 2");
}

#[test]
fn bounds_reject_world_edges_without_mutating_world() {
    let world = test_world();
    let invalid = BlockPos::new(0, PLOT_BLOCK_HEIGHT, 0);
    assert!(!world.contains_position(invalid));
    assert!(SelectionBounds::new(BlockPos::new(1, 1, 1), invalid, &world).is_err());
    assert_eq!(world.get_block(BlockPos::new(1, 1, 1)), Block::Air {});
}

#[test]
fn every_container_power_matches_the_comparator_and_survives_save() {
    let mut world = test_world();
    let position = BlockPos::new(4, 30, 4);
    for kind in [
        ContainerType::Chest,
        ContainerType::Barrel,
        ContainerType::Hopper,
        ContainerType::Furnace,
    ] {
        for power in 0..=15 {
            let item = ItemStack::container_with_ss(kind, SignalStrength::new(power).unwrap());
            let block = Block::from_name(item.item_type.get_name()).unwrap();
            world.set_block(position, Block::Air {});
            crate::interaction::place_in_world(block, &mut world, position, &item.nbt);
            assert_eq!(
                crate::redstone::comparator::get_override(block, &world, position),
                power
            );
            let entity = world.get_block_entity(position).unwrap();
            let serialized = bincode::serialize(entity).unwrap();
            let decoded: BlockEntity = bincode::deserialize(&serialized).unwrap();
            assert_eq!(bincode::serialize(&decoded).unwrap(), serialized);
        }
    }
    assert_eq!(
        bincode::serialize(&ContainerType::Furnace).unwrap(),
        0u32.to_le_bytes()
    );
    assert_eq!(
        bincode::serialize(&ContainerType::Barrel).unwrap(),
        1u32.to_le_bytes()
    );
    assert_eq!(
        bincode::serialize(&ContainerType::Hopper).unwrap(),
        2u32.to_le_bytes()
    );
    assert_eq!(
        bincode::serialize(&ContainerType::Chest).unwrap(),
        3u32.to_le_bytes()
    );
}

#[test]
fn chest_entity_survives_chunk_save_and_disappears_with_the_block() {
    let mut world = test_world();
    let position = BlockPos::new(4, 30, 4);
    let item = ItemStack::container_with_ss(ContainerType::Chest, SignalStrength::new(15).unwrap());
    let block = Block::from_name("chest").unwrap();
    crate::interaction::place_in_world(block, &mut world, position, &item.nbt);
    let chunk = world.chunks[0].save();
    let bytes = bincode::serialize(&chunk).unwrap();
    let saved = bincode::deserialize(&bytes).unwrap();
    let chunk = crate::world::storage::Chunk::load(0, 0, saved);
    let mut restored = PlotWorld::from_chunks(0, 0, vec![chunk], Default::default());
    assert_eq!(restored.get_block(position), block);
    assert_eq!(
        crate::redstone::comparator::get_override(block, &restored, position),
        15
    );
    restored.set_block(position, Block::Stone {});
    assert!(restored.get_block_entity(position).is_none());
}

pub(super) fn test_world() -> PlotWorld {
    let chunks = (0..super::super::PLOT_WIDTH)
        .flat_map(|x| {
            (0..super::super::PLOT_WIDTH).map(move |z| crate::world::storage::Chunk::empty(x, z))
        })
        .collect();
    PlotWorld::from_chunks(0, 0, chunks, Default::default())
}
