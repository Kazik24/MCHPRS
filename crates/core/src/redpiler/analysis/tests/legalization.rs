//! Small executable cases for material/context issues found in FPU and RILAX.
use super::*;
use mchprs_blocks::block_entities::{BlockEntity, ContainerType, SignalStrength};
use mchprs_blocks::items::ItemStack;

fn container_inventory(ty: ContainerType, strength: u8) -> BlockEntity {
    let item = ItemStack::container_with_ss(ty, SignalStrength::new(strength).unwrap());
    if let Some(nbt::Value::Compound(entity)) =
        item.nbt.as_ref().and_then(|nbt| nbt.get("BlockEntityTag"))
    {
        BlockEntity::from_nbt(entity).unwrap()
    } else {
        BlockEntity::Container {
            ty,
            comparator_override: 0,
            inventory: Default::default(),
        }
    }
}

#[test]
fn fixed_container_requires_matching_inert_conducting_material() {
    let mut world = empty();
    for kind in [
        ContainerType::Furnace,
        ContainerType::Barrel,
        ContainerType::Chest,
        ContainerType::Hopper,
    ] {
        let block = Block::from_name(kind.to_string().trim_start_matches("minecraft:")).unwrap();
        world.set_block(BASE, block);
        world.set_block_entity(BASE, container_inventory(kind, 4));
        assert_eq!(
            families::fixed_container(&world, BASE),
            matches!(kind, ContainerType::Furnace | ContainerType::Barrel)
        );
        world.set_block_entity(BASE, BlockEntity::Comparator { output_strength: 4 });
        assert!(!families::fixed_container(&world, BASE));
        let wrong_kind = if kind == ContainerType::Furnace {
            ContainerType::Barrel
        } else {
            ContainerType::Furnace
        };
        world.set_block_entity(BASE, container_inventory(wrong_kind, 4));
        assert!(!families::fixed_container(&world, BASE));
    }
}
