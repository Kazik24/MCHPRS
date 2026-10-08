use crate::world::World;
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
    update_comparators(world, pos);
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

pub(crate) fn update_comparators(world: &mut impl World, pos: BlockPos) {
    for face in [
        mchprs_blocks::BlockFace::North,
        mchprs_blocks::BlockFace::South,
        mchprs_blocks::BlockFace::East,
        mchprs_blocks::BlockFace::West,
    ] {
        let adjacent = pos.offset(face);
        if world.get_block(adjacent).is_solid() {
            let far = adjacent.offset(face);
            if matches!(
                world.get_block(far),
                mchprs_blocks::blocks::Block::RedstoneComparator { .. }
            ) {
                super::update(world.get_block(far), world, far, None);
            }
        }
    }
}

pub(crate) fn item_transform(
    block: mchprs_blocks::blocks::Block,
    item: &str,
) -> Option<(mchprs_blocks::blocks::Block, Option<&'static str>, i32)> {
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

pub(crate) fn oxidation_state(
    world: &impl World,
    pos: BlockPos,
    gate: f32,
    roll: f32,
) -> Option<mchprs_blocks::blocks::Block> {
    oxidation_state_with(|pos| world.get_block(pos), pos, gate, roll)
}

pub(crate) fn oxidation_state_with(
    get_block: impl Fn(BlockPos) -> mchprs_blocks::blocks::Block,
    pos: BlockPos,
    gate: f32,
    roll: f32,
) -> Option<mchprs_blocks::blocks::Block> {
    let block = get_block(pos);
    if !block.is_copper_bulb() || gate >= 0.05688889 {
        return None;
    }
    let age = block.copper_oxidation()?;
    let next = [
        "exposed_copper_bulb",
        "weathered_copper_bulb",
        "oxidized_copper_bulb",
    ]
    .get(age as usize)?;
    let mut same = 0;
    let mut older = 0;
    for dy in -4i32..=4 {
        for dz in -4i32..=4 {
            for dx in -4i32..=4 {
                let distance = dx.abs() + dy.abs() + dz.abs();
                if distance == 0 || distance > 4 {
                    continue;
                }
                let Some(other) = get_block(pos + BlockPos::new(dx, dy, dz)).copper_oxidation()
                else {
                    continue;
                };
                if other < age {
                    return None;
                }
                if other == age {
                    same += 1;
                } else {
                    older += 1;
                }
            }
        }
    }
    let fraction = (older + 1) as f32 / (older + same + 1) as f32;
    let chance = fraction * fraction * if age == 0 { 0.75 } else { 1.0 };
    (roll < chance).then(|| block.with_copper_bulb_variant(next).unwrap())
}

pub(crate) fn random_tick(world: &mut impl World, pos: BlockPos) -> bool {
    use rand::Rng;
    let mut random = rand::thread_rng();
    let Some(next) = oxidation_state(world, pos, random.gen(), random.gen()) else {
        return false;
    };
    world.set_block(pos, next);
    update(world, pos);
    super::skipping_update_surrounding_blocks(world, pos, false);
    update_comparators(world, pos);
    true
}

#[cfg(test)]
mod tests;
