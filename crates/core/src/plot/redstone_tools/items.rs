use super::ToolNotice;
use crate::player::{Gamemode, Player};
use anyhow::{bail, Context, Result};
use mchprs_blocks::block_entities::{ContainerType, SignalStrength};
use mchprs_blocks::blocks::Block;
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_network::packets::components;

pub(super) fn give_container(player: &mut Player, args: &[&str]) -> Result<()> {
    require_creative(player)?;
    let [kind, power] = args else {
        bail!("Usage: /container <chest|barrel|hopper|furnace> <0..15|a..f>");
    };
    let kind = parse_container_kind(kind)?;
    let power = power.parse::<SignalStrength>()?;
    let mut item = ItemStack::container_with_ss(kind, power);
    let mut blob = item.nbt.take().unwrap_or_default();
    components::set_tool_display(
        item.item_type.get_id() as i32,
        &mut blob,
        &format!(
            "{} · power {}",
            kind.to_string().trim_start_matches("minecraft:"),
            power.value()
        ),
        &format!("Comparator signal: {} / 15", power.value()),
    )
    .map_err(|error| anyhow::anyhow!("Cannot prepare container components: {error:?}"))?;
    item.nbt = Some(blob);
    let slot = insertion_slot(player)?;
    player.set_inventory_slot(slot, Some(item));
    ToolNotice::ItemGiven.send(player);
    Ok(())
}

pub(super) fn give_slab(player: &mut Player, args: &[&str]) -> Result<()> {
    require_creative(player)?;
    if args.len() > 1 {
        bail!("Usage: /slab [slab_type]");
    }
    let held_slot = 36 + player.selected_slot;
    if args.is_empty() {
        if let Some(held) = &player.inventory[held_slot as usize] {
            if is_slab(held.item_type) {
                let item = top_slab(held.clone())?;
                player.set_inventory_slot(held_slot, Some(item));
                ToolNotice::ItemGiven.send(player);
                return Ok(());
            }
        }
    }
    let name = args.first().copied().unwrap_or("smooth_stone_slab");
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let name = match name.ends_with("_slab") {
        true => name.to_owned(),
        false => format!("{name}_slab"),
    };
    let item_type = Item::from_name(&name).context("Unknown slab type")?;
    if !is_slab(item_type) {
        bail!("This item is not a slab");
    }
    let mut item = top_slab(ItemStack {
        item_type,
        count: 1,
        nbt: None,
    })?;
    let blob = item.nbt.as_mut().expect("top slab has components");
    components::set_tool_display(
        item_type.get_id() as i32,
        blob,
        &format!("Top {name}"),
        "Places top slabs; click an existing top slab to place beneath it.",
    )
    .map_err(|error| anyhow::anyhow!("Cannot prepare slab display: {error:?}"))?;
    let slot = insertion_slot(player)?;
    player.set_inventory_slot(slot, Some(item));
    ToolNotice::ItemGiven.send(player);
    Ok(())
}

fn top_slab(mut item: ItemStack) -> Result<ItemStack> {
    let mut blob = item.nbt.take().unwrap_or_default();
    components::set_top_slab(item.item_type.get_id() as i32, &mut blob)
        .map_err(|error| anyhow::anyhow!("Cannot prepare slab components: {error:?}"))?;
    item.nbt = Some(blob);
    Ok(item)
}

fn is_slab(item: Item) -> bool {
    Block::from_name(item.get_name()).is_some_and(|block| {
        matches!(block.property("type"), Some("bottom" | "top" | "double"))
            && block.get_name().ends_with("_slab")
    })
}

fn parse_container_kind(token: &str) -> Result<ContainerType> {
    let supported = [
        ("chest", ContainerType::Chest),
        ("barrel", ContainerType::Barrel),
        ("hopper", ContainerType::Hopper),
        ("furnace", ContainerType::Furnace),
    ];
    let token = token.strip_prefix("minecraft:").unwrap_or(token);
    let mut matches = supported
        .iter()
        .filter(|(name, _)| !token.is_empty() && name.starts_with(token));
    let kind = matches
        .next()
        .context("Unknown container type; use chest, barrel, hopper or furnace")?
        .1;
    if matches.next().is_some() {
        bail!("Ambiguous container type");
    }
    Ok(kind)
}

fn require_creative(player: &Player) -> Result<()> {
    if !matches!(player.gamemode, Gamemode::Creative) {
        bail!("Switch to creative mode first");
    }
    Ok(())
}

/// Never replace an occupied slot. Stable order: selected slot, hotbar, main inventory.
fn insertion_slot(player: &Player) -> Result<u32> {
    let selected = 36 + player.selected_slot;
    std::iter::once(selected)
        .chain(36..45)
        .chain(9..36)
        .find(|&slot| player.inventory[slot as usize].is_none())
        .context("Your inventory is full; free a slot first")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_input_is_explicit_and_validated() {
        for value in 0..=15 {
            assert_eq!(
                value.to_string().parse::<SignalStrength>().unwrap().value(),
                value
            );
        }
        for (token, value) in [("a", 10), ("f", 15)] {
            assert_eq!(token.parse::<SignalStrength>().unwrap().value(), value);
        }
        for token in ["16", "256", "-1", "A", "unknown", ""] {
            assert!(token.parse::<SignalStrength>().is_err());
        }
        assert_eq!(parse_container_kind("c").unwrap(), ContainerType::Chest);
        assert_eq!(parse_container_kind("fur").unwrap(), ContainerType::Furnace);
        assert!(parse_container_kind("").is_err());
        assert!(parse_container_kind("unknown").is_err());
    }

    #[test]
    fn top_slab_conversion_retains_count() {
        for name in ["smooth_stone_slab", "oak_slab", "cut_copper_slab"] {
            let item_type = Item::from_name(name).unwrap();
            assert!(is_slab(item_type));
            let item = top_slab(ItemStack {
                item_type,
                count: 32,
                nbt: None,
            })
            .unwrap();
            assert_eq!(item.count, 32);
            let blob = item.nbt.unwrap();
            assert!(
                matches!(blob.get("BlockStateTag"), Some(nbt::Value::Compound(state)) if state.get("type") == Some(&nbt::Value::String("top".into())))
            );
        }
        assert!(!is_slab(Item::Stone {}));
    }
}
