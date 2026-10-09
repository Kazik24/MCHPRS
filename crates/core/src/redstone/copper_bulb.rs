use crate::world::World;
use mchprs_blocks::blocks::Block;
use mchprs_blocks::BlockPos;

pub(crate) fn update(world: &mut impl World, pos: BlockPos) {
    // Dust callbacks can carry stale states; only the live latch can detect an edge.
    let block = world.get_block(pos);
    let Some((mut lit, powered)) = block.copper_bulb_state() else {
        return;
    };
    let input = super::redstone_lamp_should_be_lit(world, pos);
    if powered == input {
        return;
    }
    if input {
        lit = !lit;
        play_toggle(world, pos, lit);
    }
    world.set_block(pos, block.with_copper_bulb_state(lit, input).unwrap());
    super::skipping_update_surrounding_blocks(world, pos, false);
    super::comparator::update_far_neighbors(world, pos);
}

pub(crate) fn play_toggle(world: &mut impl World, pos: BlockPos, lit: bool) {
    world.play_sound(
        pos,
        crate::sound::event_id(if lit {
            "block.copper_bulb.turn_on"
        } else {
            "block.copper_bulb.turn_off"
        }),
        4,
        1.0,
        1.0,
    );
}

pub(crate) fn item_transform(
    block: Block,
    item: &str,
) -> Option<(Block, Option<&'static str>, i32)> {
    block.copper_bulb_state()?;
    let name = block.get_name();
    if item == "honeycomb" && !name.starts_with("waxed_") {
        return Some((
            block.with_copper_bulb_variant(&format!("waxed_{name}"))?,
            None,
            3003,
        ));
    }
    if !matches!(
        item,
        "wooden_axe" | "stone_axe" | "iron_axe" | "golden_axe" | "diamond_axe" | "netherite_axe"
    ) {
        return None;
    }
    if let Some(unwaxed) = name.strip_prefix("waxed_") {
        return Some((
            block.with_copper_bulb_variant(unwaxed)?,
            Some("item.axe.wax_off"),
            3004,
        ));
    }
    let previous = match block.copper_oxidation()? {
        1 => "copper_bulb",
        2 => "exposed_copper_bulb",
        3 => "weathered_copper_bulb",
        _ => return None,
    };
    Some((
        block.with_copper_bulb_variant(previous)?,
        Some("item.axe.scrape"),
        3005,
    ))
}

#[cfg(test)]
mod tests;
