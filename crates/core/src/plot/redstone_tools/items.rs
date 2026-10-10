use super::ToolNotice;
use crate::messages;
use crate::player::{Gamemode, Player};
use anyhow::{Context, Result, bail};
use mchprs_blocks::block_entities::{ContainerType, SignalStrength};
use mchprs_blocks::items::ItemStack;
use mchprs_network::packets::components;

pub(super) fn give_container(player: &mut Player, args: &[&str]) -> Result<()> {
    require_creative(player)?;
    let [kind, power] = args else {
        bail!(messages::USAGE_CONTAINER_CHEST_BARREL_HOPPER_FURNACE);
    };
    let kind = parse_container_kind(kind)?;
    let power = power
        .parse::<SignalStrength>()
        .map_err(|_| anyhow::anyhow!(messages::CONTAINER_INVALID_POWER))?;
    let mut item = ItemStack::container_with_ss(kind, power);
    let mut blob = item.nbt.take().unwrap_or_default();
    components::set_tool_display(
        item.item_type.get_id() as i32,
        &mut blob,
        &messages::container_tool_name(
            kind.to_string().trim_start_matches("minecraft:"),
            power.value(),
        ),
        &messages::container_tool_lore(power.value()),
    )
    .map_err(|error| anyhow::anyhow!(messages::container_components_failed(error)))?;
    item.nbt = Some(blob);
    let slot = insertion_slot(player)?;
    player.set_inventory_slot(slot, Some(item));
    ToolNotice::ItemGiven.send(player);
    Ok(())
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
        .context(messages::UNKNOWN_CONTAINER_TYPE_USE_CHEST_BARREL)?
        .1;
    if matches.next().is_some() {
        bail!(messages::AMBIGUOUS_CONTAINER_TYPE);
    }
    Ok(kind)
}

fn require_creative(player: &Player) -> Result<()> {
    if !matches!(player.gamemode, Gamemode::Creative) {
        bail!(messages::SWITCH_CREATIVE_MODE_FIRST);
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
        .context(messages::INVENTORY_FULL)
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
}
