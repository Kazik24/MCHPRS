use crate::container::item_components;
use crate::player::PlayerPos;
use crate::world::World;
use mchprs_blocks::BlockPos;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::items::{Item, ItemStack};

const MAIN_INVENTORY_START: usize = 9;
const HOTBAR_START: usize = 36;
const HOTBAR_SIZE: usize = 9;
const HOTBAR_END: usize = HOTBAR_START + HOTBAR_SIZE;

pub(super) fn within_reach(eye: PlayerPos, pos: BlockPos) -> bool {
    let distance: f64 = [(eye.x, pos.x), (eye.y, pos.y), (eye.z, pos.z)]
        .into_iter()
        .map(|(value, start)| {
            (value - value.clamp(f64::from(start), f64::from(start) + 1.0)).powi(2)
        })
        .sum();
    distance.is_finite() && distance <= 36.0 // Creative block reach (5) + Java's pick tolerance (1).
}

pub(super) fn picked_stack(
    world: &impl World,
    pos: BlockPos,
    include_data: bool,
) -> Option<ItemStack> {
    let block = world.get_block(pos);
    let name = match block {
        Block::Air | Block::MovingPiston { .. } => return None,
        Block::PistonHead { head } => {
            if head.sticky {
                "sticky_piston"
            } else {
                "piston"
            }
        }
        _ => match block.get_name() {
            "redstone_wire" => "redstone",
            "tripwire" => "string",
            "wall_torch" => "torch",
            "redstone_wall_torch" => "redstone_torch",
            "soul_wall_torch" => "soul_torch",
            name => name,
        },
    };
    let name = name
        .replace("_wall_sign", "_sign")
        .replace("_wall_hanging_sign", "_hanging_sign");
    let item_type = Item::from_name(&name)?;
    let nbt = if include_data {
        world
            .get_block_entity(pos)
            .and_then(|entity| entity.to_nbt(false))
            .map(|entity| {
                nbt::Blob::with_content(std::collections::HashMap::from([(
                    "BlockEntityTag".into(),
                    nbt::Value::Compound(entity.content),
                )]))
            })
    } else {
        None
    };
    Some(ItemStack {
        item_type,
        count: 1,
        nbt,
    })
}

// Inventory protocol order: main inventory 9..36, then hotbar 36..45.
pub(super) fn pick_into_inventory(
    inventory: &mut [Option<ItemStack>],
    selected: &mut u32,
    picked: ItemStack,
) -> Vec<usize> {
    let wanted = item_components(&picked);
    let matching = (HOTBAR_START..HOTBAR_END)
        .chain(MAIN_INVENTORY_START..HOTBAR_START)
        .find(|&slot| {
            inventory[slot].as_ref().is_some_and(|item| {
                item.item_type == picked.item_type && item_components(item) == wanted
            })
        });
    if let Some(slot) = matching.filter(|slot| *slot >= HOTBAR_START) {
        *selected = (slot - HOTBAR_START) as u32;
        return vec![];
    }

    let slot = (0..HOTBAR_SIZE)
        .map(|offset| HOTBAR_START + (*selected as usize + offset) % HOTBAR_SIZE)
        .find(|&slot| inventory[slot].is_none())
        .unwrap_or(HOTBAR_START + *selected as usize);
    *selected = (slot - HOTBAR_START) as u32;

    if let Some(source) = matching {
        inventory.swap(slot, source);
        return vec![slot, source];
    }

    let mut changed = vec![slot];
    if let Some(empty) = (MAIN_INVENTORY_START..HOTBAR_START).find(|&i| inventory[i].is_none()) {
        if inventory[slot].is_some() {
            inventory[empty] = inventory[slot].take();
            changed.push(empty);
        }
    }
    inventory[slot] = Some(picked);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(name: &str) -> ItemStack {
        ItemStack {
            item_type: Item::from_name(name).unwrap(),
            count: 1,
            nbt: None,
        }
    }

    #[test]
    fn pick_selects_existing_stacks_swaps_and_preserves_replaced_items() {
        let mut slots = vec![None; 46];
        let mut selected = 0;
        slots[40] = Some(stack("stone"));
        slots[40].as_mut().unwrap().count = 64;
        assert!(pick_into_inventory(&mut slots, &mut selected, stack("stone")).is_empty());
        assert_eq!(selected, 4);
        assert_eq!(slots[40].as_ref().unwrap().count, 64);
        slots[10] = Some(stack("redstone"));
        assert_eq!(
            pick_into_inventory(&mut slots, &mut selected, stack("redstone")),
            [41, 10]
        );
        assert!(slots[10].is_none());
        for slot in &mut slots[36..45] {
            *slot = Some(stack("stone"));
        }
        assert_eq!(
            pick_into_inventory(&mut slots, &mut selected, stack("piston")),
            [41, 9]
        );
        assert_eq!(slots[9].as_ref().unwrap().item_type, Item::Stone {});
        assert_eq!(
            slots[41].as_ref().unwrap().item_type,
            Item::from_name("piston").unwrap()
        );
        assert_eq!(slots[40].as_ref().unwrap().item_type, Item::Stone {});
    }

    #[test]
    fn pick_rejects_distant_or_nonfinite_targets() {
        let pos = BlockPos::new(0, 30, 0);
        assert!(within_reach(PlayerPos::new(7.0, 30.5, 0.5), pos));
        assert!(!within_reach(PlayerPos::new(7.01, 30.5, 0.5), pos));
        assert!(!within_reach(PlayerPos::new(f64::NAN, 30.5, 0.5), pos));
    }
}
